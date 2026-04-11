# roxy

---

[English](../README.md) · [Русский](README.ru.md) · [Українська](README.uk.md) · [Беларуская](README.be.md) · [Polski](README.pl.md) · [Deutsch](README.de.md) · **Français** · [Español](README.es.md) · [中文](README.zh-CN.md) · [日本語](README.ja.md)

---

**Serveur proxy MCP (Model Context Protocol) haute performance écrit en Rust.**

roxy relie les clients MCP (Claude Desktop, Cursor, Zed, etc.) à n'importe quel gestionnaire upstream tournant comme backend **FastCGI** (par exemple PHP-FPM) ou comme endpoint **HTTP(S)**. Rust gère tout ce qui est critique pour la performance — transport, parsing du protocole, pooling de connexions, concurrence — via le crate officiel [`rmcp`](https://crates.io/crates/rmcp). Votre gestionnaire ne manipule qu'un petit protocole JSON simplifié et renvoie les résultats.

Cela vous permet d'écrire des serveurs MCP dans **n'importe quel langage** — PHP, Python, Node, Go, Ruby — sans réimplémenter à chaque fois le framing JSON-RPC, le transport, la gestion des sessions et la négociation des capacités.

## Table des matières

- [Fonctionnalités](#fonctionnalités)
- [Installation](#installation)
  - [Homebrew (macOS et Linux)](#homebrew-macos-et-linux)
  - [Script d'installation (tout Unix)](#script-dinstallation-tout-unix)
  - [Debian / Ubuntu (.deb)](#debian--ubuntu-deb)
  - [Fedora / RHEL / openSUSE (.rpm)](#fedora--rhel--opensuse-rpm)
  - [Alpine / tout Linux (tarball statique)](#alpine--tout-linux-tarball-statique)
  - [Depuis les sources](#depuis-les-sources)
  - [Vérifier l'installation](#vérifier-linstallation)
- [Démarrage rapide](#démarrage-rapide)
- [Référence CLI](#référence-cli)
  - [Variables d'environnement](#variables-denvironnement)
- [Écriture d'un gestionnaire upstream](#écriture-dun-gestionnaire-upstream)
- [Référence du protocole upstream](#référence-du-protocole-upstream)
  - [Types de requêtes](#types-de-requêtes)
  - [Elicitation (saisie multi-étapes pour les outils)](#elicitation-saisie-multi-étapes-pour-les-outils)
  - [Réponse d'erreur](#réponse-derreur)
- [Architecture](#architecture)
- [Développement](#développement)
- [Licence](#licence)

## Fonctionnalités

- **Multi-backend** : upstreams FastCGI (TCP ou socket Unix) et HTTP(S), détection automatique à partir du format de l'URL
- **Transports** : stdio et Streamable HTTP, les deux supportés nativement via `rmcp`
- **Fonctionnalités MCP 2025-06-18** : elicitation (saisie multi-tours), sortie structurée des outils, liens de ressources dans les réponses
- **Pooling de connexions** pour FastCGI (via `deadpool`)
- **TLS via rustls** — pas de dépendance OpenSSL, builds musl entièrement statiques
- **Mise en cache des capacités** — les outils/ressources/prompts sont découverts une fois au démarrage
- **En-têtes HTTP personnalisés**, timeouts configurables, propagation des IDs de requête/session vers l'upstream

## Installation

Des binaires précompilés sont publiés à chaque release taguée pour **macOS (arm64, x86_64)** et **Linux (arm64, x86_64, musl statique)**. Choisissez la méthode adaptée à votre plateforme.

### Homebrew (macOS et Linux)

```bash
brew tap petstack/tap
brew install roxy
```

Fonctionne sur macOS (Intel et Apple Silicon) et Linux (x86_64 et arm64) avec Homebrew installé.

### Script d'installation (tout Unix)

```bash
curl -sSfL https://raw.githubusercontent.com/petstack/roxy/main/install.sh | sh
```

Le script détecte automatiquement votre système et votre architecture, télécharge le bon tarball depuis GitHub Releases, vérifie la somme de contrôle SHA256 et installe vers `/usr/local/bin/roxy` (en utilisant `sudo` si nécessaire).

Options :

```bash
# Installer une version spécifique
curl -sSfL .../install.sh | sh -s -- --version v0.1.0

# Installer dans un répertoire personnalisé (sans sudo)
curl -sSfL .../install.sh | sh -s -- --bin-dir $HOME/.local/bin
```

Les variables d'environnement `ROXY_REPO`, `ROXY_VERSION`, `ROXY_BIN_DIR` fonctionnent aussi.

### Debian / Ubuntu (.deb)

```bash
# amd64
curl -sSfLO https://github.com/petstack/roxy/releases/latest/download/roxy_0.1.0-1_amd64.deb
sudo dpkg -i roxy_0.1.0-1_amd64.deb

# arm64
curl -sSfLO https://github.com/petstack/roxy/releases/latest/download/roxy_0.1.0-1_arm64.deb
sudo dpkg -i roxy_0.1.0-1_arm64.deb
```

### Fedora / RHEL / openSUSE (.rpm)

```bash
# x86_64
sudo rpm -i https://github.com/petstack/roxy/releases/latest/download/roxy-0.1.0-1.x86_64.rpm

# aarch64
sudo rpm -i https://github.com/petstack/roxy/releases/latest/download/roxy-0.1.0-1.aarch64.rpm
```

### Alpine / tout Linux (tarball statique)

Les binaires Linux sont liés statiquement contre musl libc, ils s'exécutent donc sur **n'importe quelle** distribution Linux sans dépendances :

```bash
# Choisissez votre architecture
ARCH=x86_64   # ou aarch64
curl -sSfL https://github.com/petstack/roxy/releases/latest/download/roxy-v0.1.0-${ARCH}-unknown-linux-musl.tar.gz | tar -xz
sudo install -m 755 roxy-v0.1.0-${ARCH}-unknown-linux-musl/roxy /usr/local/bin/
```

Fonctionne sur Alpine, Debian, Ubuntu, RHEL, Arch, Amazon Linux, Void, NixOS et tout autre système avec un noyau Linux.

### Depuis les sources

Nécessite [Rust](https://rustup.rs/) (edition 2024, toolchain stable) :

```bash
git clone https://github.com/petstack/roxy
cd roxy
cargo build --release
# Le binaire est dans ./target/release/roxy
```

Ou via `cargo install` :

```bash
cargo install --git https://github.com/petstack/roxy
```

### Vérifier l'installation

```bash
roxy --version
roxy --help
```

## Démarrage rapide

roxy a besoin d'**un seul argument** : `--upstream`, pointant vers votre gestionnaire. Le type d'upstream est **détecté automatiquement** à partir du format de l'URL :

| Format URL | Type de backend |
|---|---|
| `http://...` ou `https://...` | HTTP(S) |
| `host:port` | FastCGI TCP |
| `/chemin/vers/socket` | FastCGI socket Unix |

### Exemple : backend HTTP

```bash
# Démarrez votre gestionnaire HTTP sur le port 8000 (n'importe quel langage, framework)
# Puis dirigez roxy vers lui :
roxy --upstream http://localhost:8000/mcp
```

### Exemple : backend PHP-FPM

```bash
# Démarrer PHP-FPM
php-fpm --nodaemonize \
    -d "listen=127.0.0.1:9000" \
    -d "pm=static" \
    -d "pm.max_children=4"

# Pointer roxy dessus
roxy --upstream 127.0.0.1:9000 --upstream-entrypoint /absolute/path/to/handler.php
```

### Connexion depuis un client MCP

Pour Claude Desktop ou tout client qui lance les serveurs MCP en tant que sous-processus (transport stdio — par défaut) :

```json
{
  "mcpServers": {
    "my-server": {
      "command": "roxy",
      "args": ["--upstream", "http://localhost:8000/mcp"]
    }
  }
}
```

Pour les clients réseau qui se connectent via Streamable HTTP :

```bash
roxy --transport http --port 8080 --upstream http://localhost:8000/mcp
# Le client se connecte à http://localhost:8080/mcp
```

## Référence CLI

```
roxy [OPTIONS] --upstream <UPSTREAM>
```

| Option | Par défaut | Description |
|---|---|---|
| `--upstream <URL>` | **requis** | URL du backend. Type détecté automatiquement (voir tableau ci-dessus) |
| `--transport <MODE>` | `stdio` | Transport du client MCP : `stdio` ou `http` |
| `--port <PORT>` | `8080` | Port d'écoute HTTP (uniquement avec `--transport http`) |
| `--upstream-entrypoint <PATH>` | — | `SCRIPT_FILENAME` envoyé aux backends FastCGI (requis pour PHP-FPM) |
| `--upstream-insecure` | `false` | Ignorer la vérification des certificats TLS pour les upstreams HTTPS |
| `--upstream-timeout <SECS>` | `30` | Timeout des requêtes HTTP upstream en secondes |
| `--upstream-header <HEADER>` | — | En-tête HTTP statique attaché à chaque requête vers un upstream HTTP, `Name: Value`. Répétable. Upstreams HTTP uniquement — ignoré pour FastCGI |
| `--pool-size <N>` | `16` | Taille du pool de connexions FastCGI |
| `--log-format <FORMAT>` | `pretty` | Format des logs : `pretty` ou `json` |

Le **niveau** de log est contrôlé via la variable d'environnement `RUST_LOG` :

```bash
RUST_LOG=debug roxy --upstream http://localhost:8000/mcp
RUST_LOG=roxy=debug,rmcp=info roxy --upstream ...  # filtres par module
```

### Exemple complet backend HTTP

```bash
roxy --upstream https://api.example.com/mcp \
     --upstream-header "Authorization: Bearer $TOKEN" \
     --upstream-header "X-Tenant: acme" \
     --upstream-timeout 60
```

### Exemple complet FastCGI (PHP-FPM)

```bash
# TCP
roxy --upstream 127.0.0.1:9000 --upstream-entrypoint /srv/app/handler.php

# Socket Unix
roxy --upstream /var/run/php-fpm.sock --upstream-entrypoint /srv/app/handler.php

# Transport HTTP avec upstream FastCGI
roxy --transport http --port 8080 \
     --upstream 127.0.0.1:9000 \
     --upstream-entrypoint /srv/app/handler.php
```

### Transfert des en-têtes du client

Avec `--transport http`, chaque en-tête entrant du client MCP est transmis automatiquement au backend upstream — aucune configuration n'est nécessaire. Les en-têtes hop-by-hop (RFC 7230 §6.1 : `Connection`, `Keep-Alive`, `Proxy-Authenticate`, `Proxy-Authorization`, `TE`, `Trailer`, `Transfer-Encoding`, `Upgrade`) et les en-têtes gérés par roxy lui-même (`Host`, `Content-Type`, `Content-Length`) sont filtrés. Tout le reste — `Authorization`, `Cookie`, `X-Forwarded-For`, les en-têtes personnalisés `X-*`, `mcp-session-id` — parvient à l'upstream sans modification. Ce comportement reproduit le comportement par défaut de nginx `fastcgi_pass` / `proxy_pass` et permet à votre backend upstream d'authentifier le client final (valider les tokens bearer, inspecter les cookies de session) sans que roxy ait besoin de comprendre le schéma d'authentification.

| Upstream | Forme de transfert |
|---|---|
| HTTP | Transmis comme de vrais en-têtes de requête HTTP. Les en-têtes à valeurs multiples (par ex. deux entrées `X-Forwarded-For`) sont préservés. |
| FastCGI | Traduits en paramètres CGI `HTTP_*` selon la RFC 3875 §4.1.18 — les gestionnaires PHP les lisent via `$_SERVER['HTTP_AUTHORIZATION']`, `$_SERVER['HTTP_X_FORWARDED_FOR']`, etc. Les en-têtes à valeurs multiples sont joints avec `", "` pour correspondre à la sémantique nginx `$http_*`. |

`--upstream-header` continue de fonctionner comme avant pour les upstreams HTTP — il fournit à roxy sa **propre** identité statique envers l'upstream (token de service, `X-Client-Id` fixe, etc.). Lorsqu'un en-tête transmis par le client entre en collision avec un `--upstream-header` statique portant le même nom, la valeur transmise **l'emporte** : l'identité par requête de l'appelant est plus spécifique que la valeur par défaut de roxy, ce qui correspond au comportement habituel d'un proxy inverse. `--upstream-header` est actuellement sans effet pour les upstreams FastCGI — utilisez le transfert automatique à la place.

Avec `--transport stdio`, il n'y a pas de requête HTTP entrante, donc aucun en-tête n'est transmis ; les entrées statiques `--upstream-header` s'appliquent toujours aux upstreams HTTP comme d'habitude.

### Variables d'environnement

Toutes les options CLI acceptent une variable d'environnement `ROXY_*` correspondante comme valeur de repli optionnelle. L'ordre de résolution est **CLI > env > default** : une option fournie en ligne de commande l'emporte toujours, la variable d'environnement n'est consultée qu'en l'absence de l'option, et la valeur par défaut intégrée n'est utilisée que si aucune des deux n'est présente.

| Option | Env variable | Exemple |
|---|---|---|
| `--transport` | `ROXY_TRANSPORT` | `stdio` \| `http` |
| `--port` | `ROXY_PORT` | `8080` |
| `--upstream` | `ROXY_UPSTREAM` | `http://localhost:8000/handler` |
| `--upstream-entrypoint` | `ROXY_UPSTREAM_ENTRYPOINT` | `/srv/handler.php` |
| `--upstream-insecure` | `ROXY_UPSTREAM_INSECURE` | `true` \| `false` |
| `--upstream-timeout` | `ROXY_UPSTREAM_TIMEOUT` | `30` |
| `--upstream-header` | `ROXY_UPSTREAM_HEADER` | séparés par des sauts de ligne, voir ci-dessous |
| `--pool-size` | `ROXY_POOL_SIZE` | `16` |
| `--log-format` | `ROXY_LOG_FORMAT` | `pretty` \| `json` |

#### Plusieurs valeurs upstream-header

`ROXY_UPSTREAM_HEADER` accepte plusieurs lignes d'en-tête séparées par de vrais sauts de ligne. Cela s'applique naturellement à un scalaire de bloc YAML Kubernetes :

```yaml
env:
  - name: ROXY_UPSTREAM_HEADER
    value: |-
      Authorization: Bearer xyz
      X-Trace-Id: abc
```

Depuis un shell local, utilisez le quoting `$'...'` pour que `\n` devienne un vrai saut de ligne :

```bash
ROXY_UPSTREAM_HEADER=$'Authorization: Bearer xyz\nX-Trace-Id: abc' \
  roxy --upstream https://api.example.com/mcp
```

Les lignes vides en début et en fin sont supprimées au démarrage, de sorte que les particularités du scalaire de bloc YAML `|-` ne produisent pas d'en-têtes malformés. Si `--upstream-header` est passé en CLI, `ROXY_UPSTREAM_HEADER` est entièrement ignoré — il n'y a pas de fusion des deux sources.

#### Valeurs booléennes

`ROXY_UPSTREAM_INSECURE` n'accepte que les **chaînes exactes en minuscules** `true` ou `false`. Les formes numériques (`1`, `0`) et les autres casses (`TRUE`, `True`, `YES`, `on`) sont rejetées par le parser clap (`SetTrue + env`) et provoquent une erreur au démarrage. L'option CLI `--upstream-insecure` (sans valeur) continue de fonctionner comme avant et signifie simplement `true`.

#### `RUST_LOG`

roxy respecte la variable d'environnement standard `RUST_LOG`, lue au démarrage par `tracing_subscriber::EnvFilter` ; elle est orthogonale aux variables `ROXY_*` ci-dessus et reste inchangée.

## Écriture d'un gestionnaire upstream

Votre gestionnaire reçoit de simples requêtes JSON et renvoie de simples réponses JSON. **Il ne voit jamais le JSON-RPC, le framing MCP ou l'état des sessions.** roxy traduit tout.

### Pour les backends HTTP

N'importe quel serveur HTTP qui lit du JSON depuis le corps de la requête et écrit du JSON dans la réponse fonctionnera. Exemple en Python/Flask :

```python
from flask import Flask, request, jsonify
app = Flask(__name__)

@app.post("/mcp")
def handler():
    req = request.json
    match req["type"]:
        case "discover":
            return jsonify({
                "tools": [{"name": "echo", "description": "Echo", "input_schema": {...}}],
                "resources": [],
                "prompts": [],
            })
        case "call_tool":
            return jsonify({"content": [{"type": "text", "text": req["arguments"]["message"]}]})
        case _:
            return jsonify({"error": {"code": 400, "message": "unknown"}}), 200
```

### Pour les backends FastCGI (PHP-FPM)

Un gestionnaire PHP minimal :

```php
<?php
$req = json_decode(file_get_contents('php://input'), true);
header('Content-Type: application/json');

echo json_encode(match ($req['type']) {
    'discover' => [
        'tools' => [[
            'name' => 'echo',
            'description' => 'Echoes back input',
            'input_schema' => [
                'type' => 'object',
                'properties' => ['message' => ['type' => 'string']],
                'required' => ['message'],
            ],
        ]],
        'resources' => [],
        'prompts' => [],
    ],
    'call_tool' => [
        'content' => [['type' => 'text', 'text' => $req['arguments']['message']]],
    ],
    default => ['error' => ['code' => 400, 'message' => 'unknown type']],
});
```

Voir [`examples/handler.php`](../examples/handler.php) pour un exemple complet avec plusieurs outils, sortie structurée, elicitation et liens de ressources.

## Référence du protocole upstream

Chaque requête de roxy vers votre upstream est un objet JSON avec ces champs d'enveloppe communs :

```json
{
  "type": "...",
  "session_id": "optional-uuid-or-null",
  "request_id": "uuid-per-request",
  ...
}
```

### Types de requêtes

#### `discover`

Envoyée une fois au démarrage de roxy. Votre gestionnaire doit retourner le catalogue complet des outils, ressources et prompts qu'il supporte. roxy met en cache le résultat et le sert à tous les clients MCP sans nouvelle interrogation.

```json
// Réponse
{
  "tools": [
    {
      "name": "tool_name",
      "title": "Human Name",
      "description": "Ce que ça fait",
      "input_schema": { "type": "object", "properties": {...}, "required": [...] },
      "output_schema": { "type": "object", "properties": {...} }
    }
  ],
  "resources": [
    {
      "uri": "myapp://resource-id",
      "name": "display-name",
      "title": "Human Name",
      "description": "...",
      "mime_type": "application/json"
    }
  ],
  "prompts": [
    {
      "name": "prompt_name",
      "title": "Human Name",
      "description": "...",
      "arguments": [
        { "name": "arg", "title": "Arg", "description": "...", "required": true }
      ]
    }
  ]
}
```

Les champs `title`, `description`, `mime_type`, `output_schema` sont optionnels.

#### `call_tool`

Exécute un outil par son nom. Requête :

```json
{
  "type": "call_tool",
  "name": "tool_name",
  "arguments": { "key": "value" },
  "elicitation_results": [ ... ],   // optionnel : voir la section Elicitation ci-dessous
  "context": { ... }                 // optionnel : écho d'une réponse elicit précédente
}
```

Réponse de succès (sortie texte standard) :

```json
{
  "content": [
    { "type": "text", "text": "résultat" }
  ]
}
```

Réponse de succès avec **contenu structuré** (pour les outils avec `output_schema`) :

```json
{
  "content": [{ "type": "text", "text": "5 + 3 = 8" }],
  "structured_content": { "sum": 8, "operands": { "a": 5, "b": 3 } }
}
```

Réponse de succès avec un **lien de ressource** intégré à la sortie :

```json
{
  "content": [
    { "type": "text", "text": "Réservation confirmée." },
    {
      "type": "resource_link",
      "uri": "myapp://bookings/1234",
      "name": "booking-1234",
      "title": "Réservation #1234"
    }
  ]
}
```

#### `read_resource`

Lit une ressource par URI. Requête :

```json
{ "type": "read_resource", "uri": "myapp://status" }
```

Réponse : même format `content` que `call_tool`.

#### `get_prompt`

Génère un prompt. Requête :

```json
{ "type": "get_prompt", "name": "greet", "arguments": { "name": "Alice" } }
```

Réponse : même format `content` que `call_tool`.

#### `elicitation_cancelled`

Envoyée quand le client MCP annule une elicitation (voir ci-dessous). Votre gestionnaire peut logger/nettoyer ; la réponse est ignorée.

```json
{ "type": "elicitation_cancelled", "name": "tool_name", "action": "decline", "context": {...} }
```

### Elicitation (saisie multi-étapes pour les outils)

Un outil peut **demander une saisie supplémentaire à l'utilisateur** pendant son exécution. Au premier `call_tool`, renvoyez une réponse `elicit` au lieu de `content` :

```json
{
  "elicit": {
    "message": "Quelle classe de vol ?",
    "schema": {
      "type": "object",
      "properties": {
        "class": { "type": "string", "enum": ["economy", "business", "first"] }
      },
      "required": ["class"]
    },
    "context": { "step": 1, "destination": "Tokyo" }
  }
}
```

roxy transmet l'elicitation au client MCP. Quand l'utilisateur remplit les données, roxy rappelle votre outil **une seconde fois** en passant les valeurs collectées dans `elicitation_results` et votre `context` original :

```json
{
  "type": "call_tool",
  "name": "book_flight",
  "arguments": { "destination": "Tokyo" },
  "elicitation_results": [{ "class": "business" }],
  "context": { "step": 1, "destination": "Tokyo" }
}
```

Vous pouvez enchaîner plusieurs tours d'elicitation en renvoyant d'autres `elicit` jusqu'à ce que toutes les données soient collectées.

### Réponse d'erreur

N'importe quel type de requête peut retourner une erreur au lieu d'un succès :

```json
{ "error": { "code": 404, "message": "Unknown tool: foo" } }
```

## Architecture

```
Client MCP (Claude, Cursor, Zed, ...)
       │
       │ JSON-RPC sur stdio ou Streamable HTTP
       ▼
┌──────────────┐
│    rmcp      │  Protocole MCP, transport, sessions
└──────────────┘
       │
       ▼
┌──────────────┐
│  RoxyServer  │  routage des méthodes MCP, cache de capacités
└──────────────┘
       │
       │ JSON simplifié (UpstreamEnvelope + UpstreamRequest)
       ▼
┌──────────────────────────┐
│    UpstreamExecutor      │  trait avec 2 implémentations
│  ┌────────┬───────────┐  │
│  │FastCgi │  Http     │  │
│  └────────┴───────────┘  │
└──────────────────────────┘
       │                │
       ▼                ▼
   PHP-FPM /        endpoint
   tout FastCGI     HTTP(S)
```

### Organisation des sources

```
src/
  main.rs             CLI, logging, démarrage du transport, sélection de l'executor
  lib.rs              Racine du crate bibliothèque (réexportations pour benchmarks et tests)
  config.rs           Config clap, UpstreamKind (auto-détection), FcgiAddress
  protocol.rs         Types JSON internes (UpstreamEnvelope, UpstreamRequest, ...)
  server.rs           RoxyServer : implémentation rmcp ServerHandler + cache discover
  executor/
    mod.rs            Trait UpstreamExecutor
    fastcgi.rs        FastCgiExecutor : deadpool + fastcgi-client
    http.rs           HttpExecutor : reqwest + rustls
examples/
  handler.php         Exemple complet de gestionnaire PHP avec toutes les fonctionnalités
  echo_upstream.rs    Backend HTTP echo minimal pour les tests de charge
  bench_client.rs     Client de charge end-to-end pour le profilage
```

### Décisions de conception clés

- **rmcp fait le gros du travail.** Le crate officiel `rmcp` gère toute la complexité du protocole MCP (JSON-RPC, négociation du transport, gestion des sessions). roxy n'implémente que `ServerHandler`.
- **L'upstream est enfichable.** Le trait `UpstreamExecutor` abstrait la communication avec le backend. FastCGI et HTTP sont les implémentations actuelles ; ajouter un nouveau backend (gRPC, stdio, WebSocket) se résume à implémenter un trait.
- **Les capacités sont mises en cache.** roxy appelle `discover` une fois au démarrage et garde les outils/ressources/prompts en mémoire. Les clients MCP obtiennent des réponses instantanées à `initialize` sans toucher à l'upstream.
- **Pooling de connexions pour FastCGI.** `deadpool` maintient les connexions à PHP-FPM au chaud, évitant la configuration de socket à chaque requête.
- **TLS pur Rust via rustls.** Pas d'OpenSSL, pas de bibliothèques système. Builds Linux entièrement statiques, cross-compilation facile, binaires portables.
- **L'upstream reste simple.** Votre gestionnaire ne voit jamais de JSON-RPC, d'IDs de requête (sauf comme champ opaque de l'enveloppe), d'état de session ou de framing MCP. JSON simple en entrée, JSON simple en sortie.

## Développement

### Build & tests

```bash
cargo build           # debug
cargo build --release # optimisé
cargo test            # lancer les tests
cargo clippy          # linter
cargo fmt             # formatage
```

### Exécution locale avec le gestionnaire PHP d'exemple

```bash
# Terminal 1 : démarrer PHP-FPM
php-fpm --nodaemonize -d "listen=127.0.0.1:9000" -d "pm=static" -d "pm.max_children=4"

# Terminal 2 : lancer roxy avec le gestionnaire d'exemple
cargo run -- \
    --upstream 127.0.0.1:9000 \
    --upstream-entrypoint "$(pwd)/examples/handler.php"
```

Puis connectez-vous avec n'importe quel client MCP, ou envoyez du JSON-RPC manuellement sur stdio.

### Workflow de release

Les releases taguées (`git tag vX.Y.Z && git push origin vX.Y.Z`) déclenchent `.github/workflows/release.yml`, qui :

1. Compile les binaires de release pour les quatre targets (macOS arm64/x86_64, Linux musl arm64/x86_64)
2. Les empaquette en `.tar.gz` avec des sommes de contrôle SHA256
3. Construit des paquets `.deb` et `.rpm` pour les deux architectures Linux
4. Publie une GitHub Release avec tous les artefacts
5. Met à jour la formule Homebrew dans [`petstack/homebrew-tap`](https://github.com/petstack/homebrew-tap) (si le secret `HOMEBREW_TAP_TOKEN` est défini)

Configuration du tap — voir [`packaging/homebrew/README.md`](../packaging/homebrew/README.md).

## Licence

[AGPL-3.0-only](../LICENSE). Si vous faites tourner une version modifiée de roxy comme service réseau, vous devez mettre vos modifications à disposition des utilisateurs de ce service.
