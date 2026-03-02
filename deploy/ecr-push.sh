#!/usr/bin/env bash
# Build and push Docker image to AWS ECR
# Usage: ./deploy/ecr-push.sh <aws-account-id> <region> [tag]
set -euo pipefail

ACCOUNT_ID="${1:?Usage: $0 <aws-account-id> <region> [tag]}"
REGION="${2:?Usage: $0 <aws-account-id> <region> [tag]}"
TAG="${3:-latest}"

REPO_NAME="prediction"
ECR_URI="${ACCOUNT_ID}.dkr.ecr.${REGION}.amazonaws.com/${REPO_NAME}"

echo "=== Authenticating with ECR ==="
aws ecr get-login-password --region "${REGION}" | \
    docker login --username AWS --password-stdin "${ACCOUNT_ID}.dkr.ecr.${REGION}.amazonaws.com"

echo "=== Creating repository (if needed) ==="
aws ecr describe-repositories --repository-names "${REPO_NAME}" --region "${REGION}" 2>/dev/null || \
    aws ecr create-repository --repository-name "${REPO_NAME}" --region "${REGION}"

echo "=== Building Docker image ==="
docker build -t "${REPO_NAME}:${TAG}" .

echo "=== Tagging for ECR ==="
docker tag "${REPO_NAME}:${TAG}" "${ECR_URI}:${TAG}"

echo "=== Pushing to ECR ==="
docker push "${ECR_URI}:${TAG}"

echo ""
echo "=== Done ==="
echo "Image pushed: ${ECR_URI}:${TAG}"
echo ""
echo "To deploy on EC2:"
echo "  1. SSH into your instance"
echo "  2. Update docker-compose.yml image to: ${ECR_URI}:${TAG}"
echo "     Or add:  image: ${ECR_URI}:${TAG}"
echo "  3. docker compose pull && docker compose up -d"
