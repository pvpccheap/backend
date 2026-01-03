#!/bin/bash
set -e

# Deploy script per PVPCCheap
# Executar des del servidor: ./deploy.sh

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

cd "$PROJECT_DIR"

echo "=== PVPCCheap Deploy ==="

# Verificar que existeix .env.production
if [ ! -f .env.production ]; then
    echo "ERROR: .env.production no existeix!"
    echo "Copia .env.production.example i configura els valors"
    exit 1
fi

# Carregar variables d'entorn
set -a
source .env.production
set +a

echo "1. Aturant serveis..."
podman-compose -f podman-compose.prod.yml down || true

echo "2. Construint imatge del backend..."
podman-compose -f podman-compose.prod.yml build backend

echo "3. Iniciant serveis..."
podman-compose -f podman-compose.prod.yml up -d

echo "4. Esperant que els serveis estiguin llestos..."
sleep 5

echo "5. Verificant salut..."
if curl -sf http://localhost:8080/health > /dev/null; then
    echo "✓ Backend OK"
else
    echo "✗ Backend no respon!"
    podman-compose -f podman-compose.prod.yml logs backend
    exit 1
fi

echo ""
echo "=== Deploy completat! ==="
echo "Backend escoltant a: http://localhost:8080"
echo ""
echo "Per veure logs: podman-compose -f podman-compose.prod.yml logs -f"
