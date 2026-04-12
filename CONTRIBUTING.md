# Contributing to roxy

Thanks for your interest in contributing to roxy! Whether it's a bug report, a feature request, documentation improvement, or a code change — all contributions are welcome.

## How to contribute

1. Fork the repository and clone it locally.
2. Create a branch from `main` for your change (`git checkout -b my-change`).
3. Make your changes, add tests if applicable.
4. Make sure everything passes (see [Development setup](#development-setup) below).
5. Open a pull request against `main`.

## Development setup

### Prerequisites

- [Rust](https://rustup.rs/) (stable toolchain, edition 2024)
- For FastCGI testing: PHP-FPM (or any FastCGI backend)
- For HTTP testing: any HTTP server (Python, Node, etc.)

### Build and verify

```bash
cargo build          # compile
cargo test           # run tests
cargo clippy         # lint (must pass with no warnings)
cargo fmt --check    # formatting check
```

### Running locally

#### With the example PHP handler

```bash
# Terminal 1: start PHP-FPM
php-fpm --nodaemonize \
    -d "listen=127.0.0.1:9000" \
    -d "pm=static" \
    -d "pm.max_children=4"

# Terminal 2: run roxy
cargo run -- \
    --upstream 127.0.0.1:9000 \
    --upstream-entrypoint "$(pwd)/examples/handler.php"
```

#### With a Python handler

```bash
# Install Flask
pip install flask

# Create handler.py
cat > handler.py << 'EOF'
from flask import Flask, request, jsonify

app = Flask(__name__)

@app.post("/mcp")
def handler():
    req = request.json
    match req["type"]:
        case "discover":
            return jsonify({
                "tools": [{
                    "name": "echo",
                    "description": "Echoes back the input",
                    "input_schema": {
                        "type": "object",
                        "properties": {"message": {"type": "string"}},
                        "required": ["message"],
                    },
                }],
                "resources": [],
                "prompts": [],
            })
        case "call_tool":
            return jsonify({
                "content": [{"type": "text", "text": req["arguments"]["message"]}]
            })
        case _:
            return jsonify({"error": {"code": 400, "message": "unknown type"}})

if __name__ == "__main__":
    app.run(port=8000)
EOF

# Terminal 1: start the handler
python handler.py

# Terminal 2: run roxy
cargo run -- --upstream http://localhost:8000/mcp
```

#### With a Node.js / TypeScript handler

```bash
# Create handler.mjs
cat > handler.mjs << 'EOF'
import { createServer } from "node:http";

function handle(req) {
  switch (req.type) {
    case "discover":
      return {
        tools: [{
          name: "echo",
          description: "Echoes back the input",
          input_schema: {
            type: "object",
            properties: { message: { type: "string" } },
            required: ["message"],
          },
        }],
        resources: [],
        prompts: [],
      };
    case "call_tool":
      return { content: [{ type: "text", text: req.arguments.message }] };
    default:
      return { error: { code: 400, message: "unknown type" } };
  }
}

createServer((req, res) => {
  let body = "";
  req.on("data", (c) => (body += c));
  req.on("end", () => {
    const result = handle(JSON.parse(body));
    res.writeHead(200, { "Content-Type": "application/json" });
    res.end(JSON.stringify(result));
  });
}).listen(8000, () => console.log("Listening on :8000"));
EOF

# Terminal 1: start the handler
node handler.mjs

# Terminal 2: run roxy
cargo run -- --upstream http://localhost:8000/mcp
```

## Code style

- Run `cargo fmt` before committing.
- Run `cargo clippy` and fix all warnings.
- Write tests for new functionality.
- Commit messages in English, imperative mood: "Add feature", not "Added feature".
- Keep commits focused — one logical change per commit.

## Pull requests

- Describe **what** the PR does and **why**.
- Link to a related issue if one exists.
- One logical change per PR. If your change touches unrelated things, split it into separate PRs.
- Make sure `cargo test` and `cargo clippy` pass before requesting review.

## Reporting bugs

Open an issue and include:

- roxy version (`roxy --version`)
- OS and architecture
- Upstream type (HTTP or FastCGI) and backend language
- Steps to reproduce
- Expected vs actual behavior
- Relevant logs (`RUST_LOG=debug roxy ...`)

## Feature requests

Open an issue describing your **use case**, not just the feature you want. Understanding the problem helps us find the best solution, which may differ from the one initially proposed.

## Security

If you discover a security vulnerability, **do not open a public issue**. Instead, email [diolektor@gmail.com](mailto:diolektor@gmail.com) with details. You will receive a response within 48 hours.

## License

By submitting a pull request, you agree that your contribution will be licensed under the [Apache License 2.0](LICENSE), the same license that covers the project.
