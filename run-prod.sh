#!/bin/bash
echo "Starting Production Mode (Core Services Only)..."
docker-compose up -d --build --remove-orphans
