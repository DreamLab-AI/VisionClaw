---
public: true
title: A/B Testing
---

# A/B Testing

```json-ld
{
  "@context": "https://narrativegoldmine.com/ns/v1",
  "@id": "urn:visionflow:page:a-b-testing",
  "@type": "Page",
  "title": "A/B Testing",
  "vc:slug": "a-b-testing",
  "vc:public": true,
  "vc:schemaVersion": 2,
  "vc:outboundWikilinks": []
}
```

```json-ld
{
  "@context": "https://narrativegoldmine.com/ns/v2.jsonld",
  "@id": "urn:ngm:class:a-b-testing",
  "@type": "Class",
  "label": "A/B Testing",
  "definition": "A/B testing is a controlled experimentation method that compares two or more variants by randomly assigning subjects to each and measuring a defined outcome metric. By holding all factors constant except the variant under test, it isolates causal effects and supports data-driven decisions with statistical rigour. It is widely used to optimise digital products, content and user experiences.",
  "domain": "data",
  "maturity": "mature",
  "subClassOf": [
    {
      "@id": "urn:ngm:class:data-analytics",
      "label": "Data Analytics"
    }
  ],
  "relations": {
    "partOf": [
      {
        "@id": "urn:ngm:class:data-analytics",
        "label": "Data Analytics"
      }
    ],
    "uses": [
      {
        "@id": "urn:ngm:class:statistics",
        "label": "Statistics"
      },
      {
        "@id": "urn:ngm:class:metrics",
        "label": "Metrics"
      }
    ],
    "supports": [
      {
        "@id": "urn:ngm:class:data-science",
        "label": "Data Science"
      },
      {
        "@id": "urn:ngm:class:digital-marketing",
        "label": "Digital Marketing"
      },
      {
        "@id": "urn:ngm:class:optimisation",
        "label": "Optimization"
      }
    ],
    "contrastsWith": [
      {
        "@id": "urn:ngm:class:usability-testing",
        "label": "Usability Testing"
      }
    ],
    "enables": [
      {
        "@id": "urn:ngm:class:predictive-personalization",
        "label": "Predictive Personalization"
      }
    ],
    "dependsOn": [
      {
        "@id": "urn:ngm:class:data-visualisation",
        "label": "Data Visualization"
      }
    ],
    "relatedTo": [
      {
        "@id": "urn:ngm:class:business-intelligence",
        "label": "Business Intelligence"
      },
      {
        "@id": "urn:ngm:class:user-experience",
        "label": "User Experience"
      },
      {
        "@id": "urn:ngm:class:recommendation-system",
        "label": "Recommendation System"
      },
      {
        "@id": "urn:ngm:class:monitoring-dashboard",
        "label": "Dashboard"
      }
    ]
  },
  "sameAs": [],
  "quality": 0.62,
  "provenance": {
    "attributedTo": "did:nostr:ontology-mesh",
    "generatedAt": "2026-06-15T00:00:00Z",
    "inferenceRule": "GapMaterialisation"
  }
}
```

- ### Definition
  - A/B testing is a controlled experimentation method that compares two or more variants by randomly assigning subjects to each and measuring a defined outcome metric. By holding all factors constant except the variant under test, it isolates causal effects and supports data-driven decisions with statistical rigour. It is widely used to optimise digital products, content and user experiences.
  - Related concepts: [[Data Analytics]] [[Statistics]] [[Data Science]] [[Usability Testing]] [[Metrics]]
- ### Overview
  - A/B testing applies the logic of the randomised controlled experiment to product and content decisions. Users are split at random between a control and one or more treatments, and a pre-registered metric is measured to detect whether a change produces a statistically significant improvement. Robust practice requires adequate sample sizes, guarding against peeking and multiple-comparison errors, and accounting for novelty and network effects.
- ### Key aspects
  - Random assignment of subjects to control and treatment groups
  - Pre-defined primary metric and minimum detectable effect
  - Statistical significance and confidence interval estimation
  - Sample-size and power calculation before launch
  - Guardrails against peeking, p-hacking and biased segmentation
- ### Applications
  - Website and landing-page conversion optimisation
  - Feature rollout and product experimentation
  - Email and marketing campaign tuning
  - Recommendation and ranking algorithm evaluation
  - Pricing and onboarding flow optimisation
- ### Relationships
  - subClassOf:: [[Data Analytics]]
  - partOf:: [[Data Analytics]]
  - uses:: [[Statistics]]
  - uses:: [[Metrics]]
  - supports:: [[Data Science]]
  - supports:: [[Digital Marketing]]
  - supports:: [[Optimization]]
  - contrastsWith:: [[Usability Testing]]
  - enables:: [[Predictive Personalization]]
  - dependsOn:: [[Data Visualization]]
  - relatedTo:: [[Business Intelligence]]
  - relatedTo:: [[User Experience]]
  - relatedTo:: [[Recommendation System]]
  - relatedTo:: [[Dashboard]]
- ### Provenance
  - attributedTo:: did:nostr:ontology-mesh
  - inferenceRule:: GapMaterialisation
  - quality:: 0.62
  - updated:: 2026-06-15
