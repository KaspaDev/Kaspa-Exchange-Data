#!/bin/bash
echo "Starting Development Mode (Core + SonarQube)..."
docker-compose --profile development up -d --build --remove-orphans
