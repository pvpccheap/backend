# Deploy a Hetzner amb Podman

## Requisits del servidor

- VPS CX22 o superior (2 vCPU, 4GB RAM)
- Fedora Server 40+ o Debian 12+
- Podman i podman-compose instal·lats

## Instal·lació ràpida (Fedora)

```bash
# Instal·lar dependències
sudo dnf install -y podman podman-compose git

# Clonar el projecte
cd /opt
sudo git clone https://github.com/el-teu-user/pvpccheap.git
sudo chown -R $USER:$USER pvpccheap
cd pvpccheap

# Configurar entorn
cp .env.production.example .env.production
nano .env.production  # Editar amb els teus valors

# Deploy
./deploy/deploy.sh
```

## Configurar com a servei systemd

```bash
# Copiar servei
sudo cp deploy/pvpccheap.service /etc/systemd/system/

# Habilitar i iniciar
sudo systemctl daemon-reload
sudo systemctl enable pvpccheap
sudo systemctl start pvpccheap

# Veure logs
sudo journalctl -u pvpccheap -f
```

## Configurar Nginx com a reverse proxy (recomanat)

```bash
sudo dnf install nginx certbot python3-certbot-nginx
```

Crear `/etc/nginx/conf.d/pvpccheap.conf`:

```nginx
server {
    listen 80;
    server_name api.pvpccheap.example.com;

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
}
```

```bash
# Habilitar i obtenir certificat SSL
sudo systemctl enable nginx
sudo systemctl start nginx
sudo certbot --nginx -d api.pvpccheap.example.com
```

## Actualitzar

```bash
cd /opt/pvpccheap
git pull
./deploy/deploy.sh
```

## Backups de la base de dades

```bash
# Backup manual
podman exec pvpccheap-db pg_dump -U pvpccheap pvpccheap > backup_$(date +%Y%m%d).sql

# Restaurar
cat backup.sql | podman exec -i pvpccheap-db psql -U pvpccheap pvpccheap
```

## Logs

```bash
# Tots els serveis
podman-compose -f podman-compose.prod.yml logs -f

# Només backend
podman-compose -f podman-compose.prod.yml logs -f backend

# Només postgres
podman-compose -f podman-compose.prod.yml logs -f postgres
```
