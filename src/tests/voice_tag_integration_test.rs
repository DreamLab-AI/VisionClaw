//! Integration test for voice-to-hive-mind tag system
//!
//! This test demonstrates the complete pipeline:
//! 1. User speaks command → STT → Generate tag
//! 2. Command + tag → Hive mind/agents
//! 3. Agents process and respond with tag
//! 4. Tagged response → TTS → User hears response

use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::actors::voice_commands::{SwarmIntent, SwarmVoiceResponse, VoiceCommand};
use crate::services::voice_tag_manager::{TaggedVoiceResponse, VoiceTagManager};
use crate::types::speech::SpeechOptions;
use crate::utils::time;

#[tokio::test]
async fn test_voice_tag_pipeline() {
    let mut tag_manager = VoiceTagManager::new();

    let (tts_tx, mut tts_rx) = mpsc::channel(10);
    tag_manager.set_tts_sender(tts_tx);
    let tag_manager = Arc::new(tag_manager);

    let session_id = Uuid::new_v4().to_string();
    let voice_command = VoiceCommand {
        raw_text: "spawn researcher agent".to_string(),
        parsed_intent: SwarmIntent::SpawnAgent {
            agent_type: "researcher".to_string(),
            capabilities: vec!["analysis".to_string(), "research".to_string()],
        },
        context: None,
        respond_via_voice: true,
        session_id: session_id.clone(),
        voice_tag: None,
    };

    let tagged_cmd = tag_manager
        .create_tagged_command(voice_command, true, SpeechOptions::default(), None)
        .await
        .expect("Failed to create tagged command");

    let tag = tagged_cmd.tag.clone();
    println!("Created tagged voice command with tag: {}", tag.short_id());

    assert!(tag_manager.is_tag_active(&tag.tag_id).await);

    let agent_response = TaggedVoiceResponse {
        response: SwarmVoiceResponse {
            text: "Successfully spawned researcher agent. The agent is ready to analyze data and conduct research.".to_string(),
            use_voice: true,
            metadata: None,
            follow_up: Some("What would you like the researcher to investigate?".to_string()),
            voice_tag: Some(tag.tag_id.clone()),
            is_final: Some(true),
        },
        tag: tag.clone(),
        is_final: true,
        responded_at: time::now(),
    };

    tag_manager
        .process_tagged_response(agent_response)
        .await
        .expect("Failed to process tagged response");

    let tts_response = tokio::time::timeout(tokio::time::Duration::from_secs(1), tts_rx.recv())
        .await
        .expect("Timeout waiting for TTS response")
        .expect("No TTS response received");

    assert_eq!(tts_response.tag.tag_id, tag.tag_id);
    assert!(tts_response
        .response
        .text
        .contains("Successfully spawned researcher agent"));
    assert!(tts_response.response.use_voice);
    assert!(tts_response.is_final);

    println!("Voice-to-hive-mind tag pipeline test completed successfully!");
    println!("   Tag: {}", tag.short_id());
    println!("   Response: {}", tts_response.response.text);

    assert!(!tag_manager.is_tag_active(&tag.tag_id).await);
}

#[tokio::test]
async fn test_tag_timeout_cleanup() {
    let mut tag_manager = VoiceTagManager::new();

    let (tts_tx, _tts_rx) = mpsc::channel(10);
    tag_manager.set_tts_sender(tts_tx);
    let tag_manager = Arc::new(tag_manager);

    let voice_command = VoiceCommand {
        raw_text: "help".to_string(),
        parsed_intent: SwarmIntent::Help,
        context: None,
        respond_via_voice: true,
        session_id: Uuid::new_v4().to_string(),
        voice_tag: None,
    };

    let tagged_cmd = tag_manager
        .create_tagged_command(
            voice_command,
            true,
            SpeechOptions::default(),
            Some(chrono::Duration::milliseconds(10)),
        )
        .await
        .expect("Failed to create tagged command");

    let tag = tagged_cmd.tag.clone();

    assert!(tag_manager.is_tag_active(&tag.tag_id).await);

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    tag_manager.cleanup_expired_commands().await;

    assert!(!tag_manager.is_tag_active(&tag.tag_id).await);

    println!("Tag timeout cleanup test completed successfully!");
}

#[tokio::test]
async fn test_concurrent_voice_commands() {
    let mut tag_manager = VoiceTagManager::new();

    let (tts_tx, mut tts_rx) = mpsc::channel(100);
    tag_manager.set_tts_sender(tts_tx);
    let tag_manager = Arc::new(tag_manager);

    let mut tags = Vec::new();
    for i in 0..5 {
        let voice_command = VoiceCommand {
            raw_text: format!("spawn agent {}", i),
            parsed_intent: SwarmIntent::SpawnAgent {
                agent_type: format!("agent_{}", i),
                capabilities: vec![],
            },
            context: None,
            respond_via_voice: true,
            session_id: Uuid::new_v4().to_string(),
            voice_tag: None,
        };

        let tagged_cmd = tag_manager
            .create_tagged_command(voice_command, true, SpeechOptions::default(), None)
            .await
            .expect("Failed to create tagged command");

        tags.push(tagged_cmd.tag.clone());
    }

    for tag in &tags {
        assert!(tag_manager.is_tag_active(&tag.tag_id).await);
    }

    for (i, tag) in tags.iter().enumerate() {
        let response = TaggedVoiceResponse {
            response: SwarmVoiceResponse {
                text: format!("Agent {} spawned successfully", i),
                use_voice: true,
                metadata: None,
                follow_up: None,
                voice_tag: Some(tag.tag_id.clone()),
                is_final: Some(true),
            },
            tag: tag.clone(),
            is_final: true,
            responded_at: time::now(),
        };

        tag_manager
            .process_tagged_response(response)
            .await
            .expect("Failed to process tagged response");
    }

    for _ in 0..5 {
        let tts_response = tokio::time::timeout(tokio::time::Duration::from_secs(1), tts_rx.recv())
            .await
            .expect("Timeout waiting for TTS response")
            .expect("No TTS response received");

        assert!(tts_response.response.text.contains("spawned successfully"));
        assert!(tts_response.response.use_voice);
        assert!(tts_response.is_final);
    }

    for tag in &tags {
        assert!(!tag_manager.is_tag_active(&tag.tag_id).await);
    }

    println!("Concurrent voice commands test completed successfully!");
}
