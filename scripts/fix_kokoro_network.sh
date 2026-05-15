#!/bin/bash

echo "=== Fixing Kokoro Network Configuration ==="

# Add Kokoro container to the ragflow network
echo "Adding Kokoro container to visionclaw_network network..."
docker network connect visionclaw_network friendly_dewdney

# Verify the connection
echo -e "\nVerifying network connection..."
docker inspect friendly_dewdney | grep -A 10 "Networks" | grep -E "(visionclaw_network|IPAddress)"

# Get the new IP address
KOKORO_IP=$(docker inspect friendly_dewdney -f '{{range .NetworkSettings.Networks}}{{if eq .NetworkID "b0c38a1301451c0329969ef53fdedde5221b1b05b063ad94d66017a45d3ddaa3"}}{{.IPAddress}}{{end}}{{end}}')

if [ -n "$KOKORO_IP" ]; then
    echo -e "\n✓ Kokoro is now accessible at: $KOKORO_IP:8880"
    echo "  Internal hostname: friendly_dewdney"

    # Update the settings file
    echo -e "\nUpdating settings.yaml with correct Kokoro URL..."
    sed -i "s|apiUrl: http://pedantic_morse:8880|apiUrl: http://$KOKORO_IP:8880|" //data/settings.yaml

    echo "✓ Settings updated!"

    # Test the connection
    echo -e "\nTesting Kokoro connection..."
    curl -s "http://$KOKORO_IP:8880/health" | head -5
else
    echo "✗ Failed to add Kokoro to network"
fi