# Deploy a Hetzner amb Podman Rootless

## Requisits del servidor

- VPS CX22 o superior (2 vCPU, 4GB RAM)
- Ubuntu 24.04 LTS (recomanat) o Fedora Server
- Accés SSH com a root

## Deploy ràpid

### 1. Clonar el projecte al servidor

```bash
ssh root@<IP_SERVIDOR>

# Clonar el repositori
cd /tmp
git clone https://github.com/pvpccheap/backend.git pvpccheap

# Executar setup inicial (crea usuari, instal·la dependències)
cd pvpccheap
chmod +x deploy/deploy.sh
./deploy/deploy.sh --setup
```

El setup automàticament:
- Instal·la `podman` i `podman-compose`
- Crea l'usuari `pvpccheap` (sense privilegis de root)
- Configura `loginctl enable-linger` per serveis persistents
- Copia els fitxers a `/opt/pvpccheap`
- Instal·la el servei systemd d'usuari

### 2. Configurar variables d'entorn

```bash
# Canviar a l'usuari pvpccheap
sudo -u pvpccheap -i

# Copiar i editar configuració
cd /opt/pvpccheap
cp .env.production.example .env.production
nano .env.production
```

Variables obligatòries:
- `POSTGRES_PASSWORD`: Password de PostgreSQL
- `JWT_SECRET`: Secret per JWT (genera amb `openssl rand -base64 32`)
- `GOOGLE_CLIENT_ID`: Client ID de Google OAuth
- `ESIOS_TOKEN`: Token de l'API de REE/ESIOS

### 3. Executar deploy

```bash
# Com a usuari pvpccheap
./deploy/deploy.sh
```

## Gestió del servei

```bash
# Canviar a l'usuari pvpccheap
sudo -u pvpccheap -i

# Estat del servei
systemctl --user status pvpccheap

# Reiniciar
systemctl --user restart pvpccheap

# Aturar
systemctl --user stop pvpccheap

# Veure logs
journalctl --user -u pvpccheap -f

# O amb podman-compose
cd /opt/pvpccheap
podman-compose --env-file .env.production -f podman-compose.prod.yml logs -f
```

## Actualitzar

```bash
sudo -u pvpccheap -i
cd /opt/pvpccheap
git pull
./deploy/deploy.sh
```

## Backups de la base de dades

```bash
sudo -u pvpccheap -i
cd /opt/pvpccheap

# Backup manual
podman exec pvpccheap-db pg_dump -U pvpccheap pvpccheap > backup_$(date +%Y%m%d).sql

# Restaurar
cat backup.sql | podman exec -i pvpccheap-db psql -U pvpccheap pvpccheap
```

## Firewall (UFW)

```bash
# Permetre SSH i el port del backend
sudo ufw allow ssh
sudo ufw allow 8080/tcp
sudo ufw enable
```

O a Hetzner Cloud, configura el Firewall des del panell web.

## Troubleshooting

### El servei no arrenca

```bash
# Verificar que linger està habilitat
loginctl show-user pvpccheap | grep Linger

# Si no està habilitat:
sudo loginctl enable-linger pvpccheap

# Verificar logs
journalctl --user -u pvpccheap --no-pager -n 50
```

### Podman no pot baixar imatges

```bash
# Verificar subuid/subgid
grep pvpccheap /etc/subuid /etc/subgid

# Si no existeix, afegir:
sudo sh -c 'echo "pvpccheap:100000:65536" >> /etc/subuid'
sudo sh -c 'echo "pvpccheap:100000:65536" >> /etc/subgid'

# Reiniciar sessió de l'usuari
sudo -u pvpccheap -i
podman system migrate
```

### El backend no respon

```bash
# Verificar contenidors
podman ps -a

# Veure logs del backend
podman logs pvpccheap-backend

# Verificar salut
curl http://localhost:8080/health
```

## Arquitectura

```
Internet
    │
    ▼
┌─────────────────────────────────────┐
│  VPS (Ubuntu 24.04)                 │
│                                     │
│  ┌─────────────────────────────┐   │
│  │ usuari: pvpccheap           │   │
│  │ (rootless podman)           │   │
│  │                             │   │
│  │  ┌─────────────────────┐   │   │
│  │  │ pvpccheap-backend   │   │   │
│  │  │ :8080               │◄──┼───┼── HTTP
│  │  └─────────┬───────────┘   │   │
│  │            │               │   │
│  │  ┌─────────▼───────────┐   │   │
│  │  │ pvpccheap-db        │   │   │
│  │  │ PostgreSQL 18       │   │   │
│  │  └─────────────────────┘   │   │
│  └─────────────────────────────┘   │
└─────────────────────────────────────┘
```
