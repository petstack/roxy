#!/usr/bin/env python3
"""
Example roxy upstream handler (HTTP transport, Python 3 stdlib only).

roxy translates MCP JSON-RPC into a simple envelope and POSTs it to this
server. The handler inspects ``type`` and returns a JSON payload that roxy
turns back into an MCP response.

See ``src/protocol.rs`` (``UpstreamEnvelope``, ``UpstreamRequest``,
``UpstreamCallResult``) for the authoritative Rust-side contract, and
``examples/handler.php`` / ``examples/handler.ts`` for the functionally-
identical ports in other languages.

Run::

    python3 examples/handler.py               # listens on :8000
    PORT=9001 python3 examples/handler.py     # override port

Point roxy at it::

    cargo run -- --upstream http://127.0.0.1:8000/

No third-party dependencies — only the Python 3.9+ standard library.
"""

from __future__ import annotations

import json
import logging
import os
import random
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any, Dict, List, Union

# ---------------------------------------------------------------------------
# Protocol type aliases — mirror ``src/protocol.rs``.
#
# Python doesn't enforce these at runtime but the aliases document the exact
# shapes roxy sends and expects, matching the PHP (PHPStan) and TypeScript
# variants of this example.
# ---------------------------------------------------------------------------

#: Arbitrary JSON value (replacement for the recursive ``serde_json::Value``).
JsonValue = Union[
    None, bool, int, float, str, List[Any], Dict[str, Any]
]

#: Decoded envelope from roxy. Fields present depend on the ``type`` variant.
#:
#: Common fields:
#:   - ``type``       : one of ``discover`` | ``call_tool`` | ``read_resource``
#:                       | ``get_prompt`` | ``elicitation_cancelled``
#:   - ``session_id`` : MCP session id, ``None`` under stdio transport
#:   - ``request_id`` : opaque id roxy uses to correlate replies
#:
#: Variant-specific fields:
#:   - ``call_tool``             : ``name``, ``arguments``, ``elicitation_results``, ``context``
#:   - ``read_resource``         : ``uri``
#:   - ``get_prompt``            : ``name``, ``arguments``
#:   - ``elicitation_cancelled`` : ``name``, ``action``, ``context``
Request = Dict[str, Any]

#: Standard reply body shared by ``call_tool`` / ``read_resource`` / ``get_prompt``.
#:
#: Keys:
#:   - ``content``            : list of content blocks (see below), user-visible
#:   - ``structured_content`` : optional machine-readable payload validated
#:                              against the tool's ``output_schema``
#:
#: Alternatively a handler may return:
#:   - ``{"error": {"code": int, "message": str}}`` — mapped to JSON-RPC error
#:   - ``{"elicit": {"message": str, "schema": {...}, "context": <any>}}``
#:     — mapped to an MCP ``elicitation/create`` request
Reply = Dict[str, Any]


# ---------------------------------------------------------------------------
# Handlers
# ---------------------------------------------------------------------------


def handle_discover() -> Reply:
    """Advertise every tool, resource and prompt this backend exposes.

    roxy caches the result at startup and uses it to serve ``tools/list``,
    ``resources/list`` and ``prompts/list`` without re-calling us.
    """
    return {
        # Tool catalog. Each entry:
        #   name          : stable machine id — used by call_tool
        #   title         : human label shown by MCP clients
        #   description   : free-form description shown by clients
        #   input_schema  : JSON Schema for ``arguments``
        #   output_schema : optional JSON Schema for ``structured_content``
        "tools": [
            {
                "name": "echo",                                    # tool id — unique per backend
                "title": "Echo Message",                           # label in MCP clients
                "description": "Echoes back the input message",    # shown next to the title
                "input_schema": {
                    "type": "object",                              # JSON Schema root type
                    "properties": {
                        "message": {
                            "type": "string",                      # JSON Schema type
                            "description": "The message to echo",  # field-level hint
                        },
                    },
                    "required": ["message"],                       # keys that MUST be present
                },
            },
            {
                "name": "add",
                "title": "Add Numbers",
                "description": "Adds two numbers and returns structured result",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        # ``number`` covers both int and float in JSON Schema.
                        "a": {"type": "number", "description": "First number"},
                        "b": {"type": "number", "description": "Second number"},
                    },
                    "required": ["a", "b"],
                },
                # Optional — lets clients validate ``structured_content`` and
                # render it with field names / types.
                "output_schema": {
                    "type": "object",
                    "properties": {
                        "sum": {"type": "number"},   # the computed sum
                        "operands": {                # echoes the inputs for audit
                            "type": "object",
                            "properties": {
                                "a": {"type": "number"},
                                "b": {"type": "number"},
                            },
                        },
                    },
                },
            },
            {
                "name": "book_flight",
                "title": "Book a Flight",
                "description": "Books a flight with elicitation for missing details",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "destination": {
                            "type": "string",
                            "description": "Flight destination",
                        },
                    },
                    "required": ["destination"],
                },
            },
        ],
        # Resource catalog. Each entry:
        #   uri         : identifier clients pass back in ``read_resource``
        #   name        : stable machine id
        #   title       : human label
        #   description : free-form description
        #   mime_type   : IANA mime type of the resource body
        "resources": [
            {
                "uri": "roxy://status",             # opaque, scheme is up to the backend
                "name": "server-status",            # stable machine id
                "title": "Server Status",           # shown by clients
                "description": "Current server status",  # shown by clients
                "mime_type": "application/json",    # how the client should render the body
            },
        ],
        # Prompt catalog. Each entry:
        #   name        : stable machine id — used by get_prompt
        #   title       : human label
        #   description : free-form description
        #   arguments   : list of {name, title, description, required}
        "prompts": [
            {
                "name": "greet",
                "title": "Greeting",
                "description": "Generate a greeting",
                "arguments": [
                    {
                        "name": "name",                # argument key passed in get_prompt.arguments
                        "title": "Person Name",        # human label
                        "description": "Name to greet",
                        "required": True,              # clients prompt the user before sending
                    },
                ],
            },
        ],
    }


def handle_call_tool(request: Request) -> Reply:
    """Dispatch a ``call_tool`` request to the matching sub-handler."""
    name: str = request.get("name", "")
    args: Dict[str, Any] = request.get("arguments") or {}
    # Ordered list of previous elicitation replies, oldest first. Empty on the
    # first call; populated on each follow-up while a multi-step elicitation
    # flow is in progress.
    elicitation_results: List[Dict[str, Any]] = request.get("elicitation_results") or []
    # Opaque value this handler returned in a previous ``elicit`` reply. roxy
    # threads it back verbatim so the handler can restore its own state.
    context: Any = request.get("context")

    if name == "echo":
        message = args.get("message", "")
        return {
            "content": [
                {
                    "type": "text",                # discriminator for the content block kind
                    "text": str(message),          # the string to display
                },
            ],
        }

    if name == "add":
        return handle_add(args)

    if name == "book_flight":
        return handle_book_flight(args, elicitation_results, context)

    return {
        "error": {
            "code": 404,
            "message": f"Unknown tool: {name}",
        },
    }


def handle_add(args: Dict[str, Any]) -> Reply:
    """Implementation of the ``add`` tool."""
    a = args.get("a", 0)
    b = args.get("b", 0)
    total = a + b

    return {
        # Human-readable rendering of the result.
        "content": [
            {
                "type": "text",
                "text": f"{a} + {b} = {total}",
            },
        ],
        # Machine-readable result — matches the tool's ``output_schema``.
        "structured_content": {
            "sum": total,                     # matches output_schema.properties.sum
            "operands": {"a": a, "b": b},     # echoes the inputs for audit
        },
    }


def handle_book_flight(
    args: Dict[str, Any],
    elicitation_results: List[Dict[str, Any]],
    _context: Any,
) -> Reply:
    """Multi-turn example.

    First call returns an ``elicit`` asking for flight class; the second call
    (with the elicitation reply) completes the booking.
    """
    destination = args.get("destination", "Unknown")

    # First round: no prior elicitation replies — ask the client for the class.
    if not elicitation_results:
        return {
            "elicit": {
                "message": f"Select flight class for {destination}",  # prompt shown to user
                # JSON Schema describing the expected reply. Clients render
                # the fields as a form.
                "schema": {
                    "type": "object",
                    "properties": {
                        "class": {
                            "type": "string",
                            "title": "Flight Class",
                            "enum": ["economy", "business", "first"],  # allowed values
                        },
                    },
                    "required": ["class"],
                },
                # Opaque handler-side bookkeeping. roxy replays it on the
                # follow-up call. Use it for whatever you need to resume — here
                # it records which step of the flow we're on.
                "context": {
                    "destination": destination,
                    "step": 1,
                },
            },
        }

    # Second round: the reply carries the class the user picked.
    reply = elicitation_results[0] if elicitation_results else {}
    cls = reply.get("class", "economy")
    booking_id = random.randint(1000, 9999)

    return {
        "content": [
            # Plain-text summary.
            {
                "type": "text",
                "text": f"Booked {cls} flight to {destination}. Booking #{booking_id}",
            },
            # A ``resource_link`` lets the client fetch details via read_resource.
            {
                "type": "resource_link",                        # discriminator
                "uri": f"roxy://bookings/{booking_id}",          # target of the follow-up read_resource
                "name": f"booking-{booking_id}",                 # stable machine id
                "title": f"Booking #{booking_id}",               # human label
            },
        ],
        "structured_content": {
            "booking_id": booking_id,    # generated booking id
            "destination": destination,  # echoed from the original arguments
            "class": cls,                # value picked by the user during elicitation
        },
    }


def handle_read_resource(request: Request) -> Reply:
    """Answer a ``read_resource`` request."""
    uri: str = request.get("uri", "")

    if uri == "roxy://status":
        return {
            # Resource bodies ride in the same ``content`` array as tool output.
            "content": [
                {
                    "type": "text",
                    # Stringified JSON — declared as application/json in discover.
                    "text": json.dumps(
                        {
                            "status": "running",       # fixed in this demo
                            "python": sys.version.split()[0],  # observed at runtime
                        }
                    ),
                },
            ],
        }

    return {
        "error": {
            "code": 404,
            "message": f"Unknown resource: {uri}",
        },
    }


def handle_get_prompt(request: Request) -> Reply:
    """Answer a ``get_prompt`` request."""
    name: str = request.get("name", "")
    args: Dict[str, Any] = request.get("arguments") or {}

    if name == "greet":
        person_name = args.get("name", "World")
        return {
            "content": [
                {
                    "type": "text",
                    "text": f"Hello, {person_name}! How can I help you today?",
                },
            ],
        }

    return {
        "error": {
            "code": 404,
            "message": f"Unknown prompt: {name}",
        },
    }


def handle_elicitation_cancelled(request: Request) -> Reply:
    """Notification that the user declined or cancelled a pending elicitation.

    roxy doesn't wait for a meaningful body — it just wants the handler to
    know so any server-side state tied to the flow can be cleaned up.
    """
    name: str = request.get("name", "")     # tool the elicitation belonged to
    action: str = request.get("action", "")  # 'decline' or 'cancel'
    logging.info("Elicitation %s for tool: %s", action, name)
    return {"ok": True}


def dispatch(request: Request) -> Reply:
    """Route an envelope to the matching handler based on ``type``."""
    request_type = request.get("type")

    if request_type == "discover":
        return handle_discover()
    if request_type == "call_tool":
        return handle_call_tool(request)
    if request_type == "read_resource":
        return handle_read_resource(request)
    if request_type == "get_prompt":
        return handle_get_prompt(request)
    if request_type == "elicitation_cancelled":
        return handle_elicitation_cancelled(request)

    return {
        "error": {
            "code": 400,
            "message": f"Unknown request type: {request_type}",
        },
    }


# ---------------------------------------------------------------------------
# HTTP server
# ---------------------------------------------------------------------------


class RoxyHandler(BaseHTTPRequestHandler):
    """Minimal POST-only HTTP handler that decodes JSON and dispatches."""

    # Silence the default per-request stderr logging; use the logging module
    # instead so the output plays well with roxy's own logs.
    def log_message(self, format: str, *args: Any) -> None:  # noqa: A002
        logging.debug("%s - %s", self.address_string(), format % args)

    def _send_json(self, status: int, payload: Reply) -> None:
        body = json.dumps(payload).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def do_POST(self) -> None:  # noqa: N802 — required name from BaseHTTPRequestHandler
        length = int(self.headers.get("Content-Length", "0") or "0")
        raw = self.rfile.read(length) if length > 0 else b""

        try:
            body = json.loads(raw.decode("utf-8")) if raw else None
        except (UnicodeDecodeError, json.JSONDecodeError):
            self._send_json(400, {"error": {"code": 400, "message": "Invalid JSON body"}})
            return

        if not isinstance(body, dict) or not isinstance(body.get("type"), str):
            self._send_json(
                400,
                {"error": {"code": 400, "message": "Invalid request: missing type field"}},
            )
            return

        try:
            reply = dispatch(body)
        except Exception as exc:  # noqa: BLE001 — any handler error becomes a 500
            logging.exception("handler failed")
            self._send_json(500, {"error": {"code": 500, "message": str(exc)}})
            return

        self._send_json(200, reply)

    def do_GET(self) -> None:  # noqa: N802
        self._send_json(405, {"error": {"code": 405, "message": "Method not allowed"}})


def main() -> None:
    logging.basicConfig(
        level=os.environ.get("LOG_LEVEL", "INFO"),
        format="%(asctime)s %(levelname)s %(message)s",
    )

    port = int(os.environ.get("PORT", "8000"))
    httpd = ThreadingHTTPServer(("127.0.0.1", port), RoxyHandler)
    logging.info("roxy example upstream listening on http://127.0.0.1:%d/", port)
    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        logging.info("shutting down")
        httpd.server_close()


if __name__ == "__main__":
    main()
