#!/bin/bash
set -e

# Deploy script per PVPCCheap (Rootless Podman)
# Executar des del servidor: ./deploy/deploy.sh

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
APP_USER="${APP_USER:-pvpccheap}"
APP_DIR="/opt/pvpccheap"

# Colors per output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

info() { echo -e "${GREEN}[INFO]${NC} $1"; }
warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
error() { echo -e "${RED}[ERROR]${NC} $1"; exit 1; }

echo "=== PVPCCheap Deploy (Rootless) ==="
echo ""

# Detectar si estem executant com a root
if [ "$EUID" -eq 0 ]; then
    RUNNING_AS_ROOT=true
else
    RUNNING_AS_ROOT=false
fi

# === SETUP INICIAL (només com a root) ===
setup_system() {
    if [ "$RUNNING_AS_ROOT" = false ]; then
        error "El setup inicial requereix permisos de root. Executa: sudo $0 --setup"
    fi

    info "Configurant sistema per primera vegada..."

    # Instal·lar dependències (Ubuntu/Debian)
    if command -v apt-get &> /dev/null; then
        info "Instal·lant dependències (apt)..."
        apt-get update
        apt-get install -y podman podman-compose curl git
    # Fedora/RHEL
    elif command -v dnf &> /dev/null; then
        info "Instal·lant dependències (dnf)..."
        dnf install -y podman podman-compose curl git
    else
        warn "Gestor de paquets no reconegut. Assegura't de tenir podman i podman-compose instal·lats."
    fi

    # Crear usuari si no existeix
    if ! id "$APP_USER" &>/dev/null; then
        info "Creant usuari '$APP_USER'..."
        useradd -r -m -s /bin/bash "$APP_USER"
    else
        info "Usuari '$APP_USER' ja existeix"
    fi

    # Crear directori de l'aplicació
    if [ ! -d "$APP_DIR" ]; then
        info "Creant directori $APP_DIR..."
        mkdir -p "$APP_DIR"
    fi

    # Copiar fitxers del projecte
    info "Copiant fitxers del projecte..."
    cp -r "$PROJECT_DIR"/* "$APP_DIR/"
    chown -R "$APP_USER:$APP_USER" "$APP_DIR"

    # Habilitar linger per l'usuari (permet serveis sense login)
    info "Habilitant linger per '$APP_USER'..."
    loginctl enable-linger "$APP_USER"

    # Crear directori per serveis systemd d'usuari
    USER_SYSTEMD_DIR="/home/$APP_USER/.config/systemd/user"
    mkdir -p "$USER_SYSTEMD_DIR"

    # Copiar servei systemd
    info "Instal·lant servei systemd..."
    cp "$SCRIPT_DIR/pvpccheap.service" "$USER_SYSTEMD_DIR/"
    chown -R "$APP_USER:$APP_USER" "/home/$APP_USER/.config"

    # Configurar subuid/subgid per rootless podman
    if ! grep -q "^$APP_USER:" /etc/subuid 2>/dev/null; then
        info "Configurant subuid/subgid per rootless podman..."
        echo "$APP_USER:100000:65536" >> /etc/subuid
        echo "$APP_USER:100000:65536" >> /etc/subgid
    fi

    echo ""
    info "=== Setup del sistema completat! ==="
    echo ""
    echo "Ara has de:"
    echo "1. Crear el fitxer de configuració:"
    echo "   sudo -u $APP_USER cp $APP_DIR/.env.production.example $APP_DIR/.env.production"
    echo "   sudo -u $APP_USER nano $APP_DIR/.env.production"
    echo ""
    echo "2. Executar el deploy com a usuari $APP_USER:"
    echo "   sudo -u $APP_USER $APP_DIR/deploy/deploy.sh"
    echo ""
}

# === DEPLOY (com a usuari normal) ===
deploy() {
    cd "$APP_DIR"

    # Verificar que existeix .env.production
    if [ ! -f .env.production ]; then
        error ".env.production no existeix! Copia .env.production.example i configura els valors"
    fi

    info "1. Aturant serveis existents..."
    podman-compose --env-file .env.production -f podman-compose.prod.yml down 2>/dev/null || true

    info "2. Construint imatge del backend..."
    podman-compose --env-file .env.production -f podman-compose.prod.yml build backend

    info "3. Iniciant serveis..."
    podman-compose --env-file .env.production -f podman-compose.prod.yml up -d

    info "4. Esperant que els serveis estiguin llestos..."
    sleep 10

    info "5. Verificant salut..."
    if curl -sf http://localhost:8080/health > /dev/null; then
        echo -e "${GREEN}✓ Backend OK${NC}"
    else
        echo -e "${RED}✗ Backend no respon!${NC}"
        podman-compose --env-file .env.production -f podman-compose.prod.yml logs backend
        exit 1
    fi

    # Habilitar servei systemd d'usuari
    info "6. Configurant servei systemd..."
    systemctl --user daemon-reload
    systemctl --user enable pvpccheap 2>/dev/null || true

    echo ""
    info "=== Deploy completat! ==="
    echo ""
    echo "Backend escoltant a: http://localhost:8080"
    echo ""
    echo "Comandes útils:"
    echo "  Veure logs:     podman-compose --env-file .env.production -f podman-compose.prod.yml logs -f"
    echo "  Reiniciar:      systemctl --user restart pvpccheap"
    echo "  Estat servei:   systemctl --user status pvpccheap"
    echo ""
}

# === MAIN ===
case "${1:-}" in
    --setup|-s)
        setup_system
        ;;
    --help|-h)
        echo "Ús: $0 [opció]"
        echo ""
        echo "Opcions:"
        echo "  --setup, -s    Setup inicial del sistema (requereix root)"
        echo "  --help, -h     Mostra aquest missatge"
        echo "  (sense opció)  Executa el deploy"
        echo ""
        echo "Primer cop:"
        echo "  1. sudo $0 --setup"
        echo "  2. Configurar .env.production"
        echo "  3. sudo -u pvpccheap $0"
        ;;
    *)
        if [ "$RUNNING_AS_ROOT" = true ]; then
            warn "Estàs executant com a root. El deploy s'hauria d'executar com a usuari '$APP_USER'."
            warn "Executa: sudo -u $APP_USER $0"
            echo ""
            read -p "Vols continuar igualment com a root? [y/N] " -n 1 -r
            echo
            if [[ ! $REPLY =~ ^[Yy]$ ]]; then
                exit 1
            fi
        fi
        deploy
        ;;
esac
