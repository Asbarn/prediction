#!/usr/bin/env bash
# EC2 bootstrap / user-data script for prediction soak test
# Usage: Run as root on a fresh Amazon Linux 2023 or Ubuntu 22.04 instance
set -euo pipefail

APP_DIR="/opt/prediction"
COMPOSE_VERSION="2.24.5"

echo "=== Installing Docker ==="
if command -v apt-get &>/dev/null; then
    # Ubuntu / Debian
    apt-get update
    apt-get install -y docker.io curl unzip
    systemctl enable --now docker
elif command -v yum &>/dev/null; then
    # Amazon Linux
    yum install -y docker
    systemctl enable --now docker
fi

echo "=== Installing Docker Compose ==="
mkdir -p /usr/local/lib/docker/cli-plugins
curl -SL "https://github.com/docker/compose/releases/download/v${COMPOSE_VERSION}/docker-compose-linux-$(uname -m)" \
    -o /usr/local/lib/docker/cli-plugins/docker-compose
chmod +x /usr/local/lib/docker/cli-plugins/docker-compose

echo "=== Creating app directory ==="
mkdir -p "${APP_DIR}"/{config,spread_logs,settlement_logs,paper_trades,state,logs}

echo "=== Setup complete ==="
echo ""
echo "Next steps:"
echo "  1. Copy config files:  scp config/*.toml ec2-user@<host>:${APP_DIR}/config/"
echo "  2. Copy .env file:     scp .env ec2-user@<host>:${APP_DIR}/.env"
echo "  3. Copy compose file:  scp docker-compose.yml ec2-user@<host>:${APP_DIR}/"
echo "  4. Pull image:         cd ${APP_DIR} && docker compose pull"
echo "     Or build locally:   cd ${APP_DIR} && docker compose build"
echo "  5. Start:              cd ${APP_DIR} && docker compose up -d"
echo "  6. Check health:       curl http://localhost:9001/health"
echo "  7. View logs:          cd ${APP_DIR} && docker compose logs -f"
