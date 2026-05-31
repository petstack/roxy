---
title: roxy documentation
description: roxy is a high-performance MCP gateway, written in Rust, that connects any existing backend — in any language — to the Model Context Protocol over HTTP(S) or FastCGI.
---

# roxy documentation

A friendly, illustrated guide for people who want to use **roxy** — no insider knowledge required.

roxy is a gateway that lets you write an MCP server as a plain "JSON in, JSON out"
handler in any language. It speaks the Model Context Protocol to AI clients
(Claude Desktop, Cursor, Zed, …) and forwards a small, stable JSON contract to
your backend over HTTP(S) or FastCGI.

## Contents

1. [Introduction](introduction.md) — what roxy is, the vocabulary, and how it works.
2. [Installation & first run](installation.md) — install roxy and connect your first backend in two steps.
3. [Configuration](configuration.md) — transports, backend detection, CLI flags, and environment variables.
4. [The backend API](backend-api.md) — the full JSON contract your backend must fulfill.
5. [Operations](operations.md) — header forwarding, logging, and error messages.
6. [Examples & FAQ](examples-and-faq.md) — runnable example backends, end-to-end configs, and common questions.
7. [Architecture](architecture.md) — a map of the roxy source tree and key dependencies, for contributors and code assistants.

See also: [Benchmarks](../BENCHMARKS.md).
