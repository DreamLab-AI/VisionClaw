#!/bin/bash
set -e

# Define paths
REPO_ROOT="$(dirname "$(dirname "$(readlink -f "$0")")")"
DOCKER_DIR="$REPO_ROOT/multi-agent-docker"
ENV_FILE="$DOCKER_DIR/.env"
EXAMPLE_ENV="$DOCKER_DIR/.env.example"

echo "Provisioning Multi-Agent Docker Environment..."

# 1. Verify Directories
if [ ! -d "$DOCKER_DIR" ]; then
    echo "Error: Directory $DOCKER_DIR not found."
    exit 1
fi

# 2. Check for SSH Keys
echo "Checking SSH keys..."
if [ -d "$HOME/.ssh" ]; then
    echo "  Found ~/.ssh directory."
else
    echo "  Warning: ~/.ssh directory not found. SSH access to container may be limited."
fi

# 3. Check for Claude Configuration
echo "Checking Claude configuration..."
if [ -d "$HOME/.claude" ]; then
    echo "  Found ~/.claude directory."
elif [ -d "$HOME/.config/claude" ]; then
    echo "  Found ~/.config/claude directory."
else
    echo "  Warning: Host Claude configuration not found. You may need to sign in within the container."
fi

# 4. Create Docker Network
echo "Ensuring visionclaw_network network exists..."
if ! docker network ls | grep -q "visionclaw_network"; then
    docker network create visionclaw_network
    echo "  Created visionclaw_network network."
else
    echo "  visionclaw_network network already exists."
fi

# 5. Set up .env file
echo "Setting up .env file..."
if [ ! -f "$ENV_FILE" ]; then
    if [ -f "$EXAMPLE_ENV" ]; then
        cp "$EXAMPLE_ENV" "$ENV_FILE"
        echo "  Created .env from .env.example."
    else
        echo "Error: .env.example not found in $DOCKER_DIR."
        exit 1
    fi
else
    echo "  .env file already exists."
fi

# Helper function to update .env
update_env() {
    local key=$1
    local value=$2
    if [ -n "$value" ]; then
        # Escape special characters in value for sed
        local escaped_value=$(echo "$value" | sed 's/[\/&]/\\&/g')
        if grep -q "^$key=" "$ENV_FILE"; then
            sed -i "s/^$key=.*/$key=$escaped_value/" "$ENV_FILE"
        else
            echo "$key=$value" >> "$ENV_FILE"
        fi
        echo "  Updated $key"
    fi
}

# 6. Populate Keys from Host Environment
echo "Populating keys from host environment..."

# Function to extract key from JSON file
extract_key_from_json() {
    local file=$1
    local key_name=$2
    if [ -f "$file" ]; then
        if command -v jq >/dev/null 2>&1; then
            jq -r ".$key_name // empty" "$file"
        else
            # Simple grep/sed fallback for simple JSON
            grep -o "\"$key_name\": *\"[^\"]*\"" "$file" | sed 's/.*: *"\(.*\)"/\1/'
        fi
    fi
}

# Anthropic
FOUND_KEY=""
if [ -n "$ANTHROPIC_API_KEY" ]; then
    FOUND_KEY="$ANTHROPIC_API_KEY"
else
    # Try to find it in config files
    # Standard Claude CLI config
    CLAUDE_CLI_CONFIG="$HOME/.config/claude/config.json"
    FOUND_KEY=$(extract_key_from_json "$CLAUDE_CLI_CONFIG" "apiKey")

    if [ -z "$FOUND_KEY" ]; then
         FOUND_KEY=$(extract_key_from_json "$CLAUDE_CLI_CONFIG" "api_key")
    fi

    # Try alternate location
    if [ -z "$FOUND_KEY" ]; then
         FOUND_KEY=$(extract_key_from_json "$HOME/.claude/config.json" "apiKey")
    fi

    if [ -z "$FOUND_KEY" ]; then
         FOUND_KEY=$(extract_key_from_json "$HOME/.claude/config.json" "api_key")
    fi

    # Try auth.json (sometimes used)
    if [ -z "$FOUND_KEY" ]; then
         FOUND_KEY=$(extract_key_from_json "$HOME/.config/claude/auth.json" "apiKey")
    fi

    # Try Desktop config
    if [ -z "$FOUND_KEY" ]; then
         FOUND_KEY=$(extract_key_from_json "$HOME/.config/claude/claude_desktop_config.json" "apiKey")
    fi
    if [ -z "$FOUND_KEY" ]; then
         FOUND_KEY=$(extract_key_from_json "$HOME/.config/claude/claude_desktop_config.json" "api_key")
    fi
fi

if [ -n "$FOUND_KEY" ]; then
    update_env "ANTHROPIC_API_KEY" "$FOUND_KEY"
    echo "  Configured ANTHROPIC_API_KEY."

    # Also set ZAI_ANTHROPIC_API_KEY if not set separately in env
    if [ -z "$ZAI_ANTHROPIC_API_KEY" ]; then
        # Only update if it's currently the placeholder
        if grep -q "ZAI_ANTHROPIC_API_KEY=your-zai-anthropic-api-key-here" "$ENV_FILE"; then
             update_env "ZAI_ANTHROPIC_API_KEY" "$FOUND_KEY"
        fi
    fi
else
    echo "  Warning: ANTHROPIC_API_KEY not found in host environment or Claude config files."
    echo "           Checked: ~/.config/claude/config.json, ~/.claude/config.json"
fi

# OpenAI
if [ -n "$OPENAI_API_KEY" ]; then
    update_env "OPENAI_API_KEY" "$OPENAI_API_KEY"
fi
if [ -n "$OPENAI_ORG_ID" ]; then
    update_env "OPENAI_ORG_ID" "$OPENAI_ORG_ID"
fi

# Gemini
if [ -n "$GOOGLE_GEMINI_API_KEY" ]; then
    update_env "GOOGLE_GEMINI_API_KEY" "$GOOGLE_GEMINI_API_KEY"
fi

# GitHub
if [ -n "$GITHUB_TOKEN" ]; then
    update_env "GITHUB_TOKEN" "$GITHUB_TOKEN"
fi

# Perplexity
if [ -n "$PERPLEXITY_API_KEY" ]; then
    update_env "PERPLEXITY_API_KEY" "$PERPLEXITY_API_KEY"
fi

# Management API Key
if ! grep -q "^MANAGEMENT_API_KEY=" "$ENV_FILE" || grep -q "MANAGEMENT_API_KEY=generate-a-secure-random-key-here" "$ENV_FILE"; then
    if command -v openssl >/dev/null 2>&1; then
        RANDOM_KEY=$(openssl rand -hex 16)
        update_env "MANAGEMENT_API_KEY" "$RANDOM_KEY"
        echo "  Generated new MANAGEMENT_API_KEY."
    else
        echo "  Warning: openssl not found, skipping random key generation."
    fi
fi

# ZAI API Key
if ! grep -q "^ZAI_API_KEY=" "$ENV_FILE" || grep -q "ZAI_API_KEY=your-zai-api-key-here" "$ENV_FILE"; then
    if command -v openssl >/dev/null 2>&1; then
        RANDOM_ZAI_KEY=$(openssl rand -hex 16)
        update_env "ZAI_API_KEY" "$RANDOM_ZAI_KEY"
        echo "  Generated new ZAI_API_KEY."
    fi
fi


echo "Provisioning complete. You can now run:"
echo "  cd multi-agent-docker && ./build-unified.sh"
echo "  OR"
echo "  docker compose -f multi-agent-docker/docker-compose.unified.yml up -d"
