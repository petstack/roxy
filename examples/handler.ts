/**
 * Example roxy upstream handler (HTTP transport, Node.js / TypeScript).
 *
 * roxy translates MCP JSON-RPC into a simple envelope and POSTs it to this
 * server. The handler inspects `type` and returns a JSON payload that roxy
 * turns back into an MCP response.
 *
 * See `src/protocol.rs` (`UpstreamEnvelope`, `UpstreamRequest`,
 * `UpstreamCallResult`) for the authoritative Rust-side contract, and
 * `examples/handler.php` for the functionally-identical PHP port.
 *
 * Run:
 *   node --experimental-strip-types examples/handler.ts
 *   # or: npx tsx examples/handler.ts
 *
 * Point roxy at it:
 *   cargo run -- --upstream http://127.0.0.1:8000/
 */

import { createServer, type IncomingMessage, type ServerResponse } from "node:http";

// ---------------------------------------------------------------------------
// Protocol types — mirror `src/protocol.rs`.
// ---------------------------------------------------------------------------

/** Fields present on every request from roxy regardless of `type`. */
interface UpstreamEnvelopeBase {
  /** MCP session id. `null` under stdio transport. */
  session_id?: string | null;
  /** Opaque id roxy uses to correlate replies. */
  request_id?: string;
}

/** Ask the backend to advertise its tools / resources / prompts. */
interface DiscoverRequest extends UpstreamEnvelopeBase {
  type: "discover";
}

/** Invoke a tool by name. May carry prior elicitation state. */
interface CallToolRequest extends UpstreamEnvelopeBase {
  type: "call_tool";
  /** Tool id — must match one advertised in `discover`. */
  name: string;
  /** Tool arguments, shape defined by the tool's `input_schema`. */
  arguments?: Record<string, unknown>;
  /**
   * Ordered list of prior elicitation replies, oldest first. Empty on the
   * first call; populated on each follow-up while a multi-step elicitation
   * flow is in progress.
   */
  elicitation_results?: Array<Record<string, unknown>>;
  /**
   * Opaque value this handler returned in a previous `elicit` reply. roxy
   * threads it back verbatim so the handler can restore its own state.
   */
  context?: unknown;
}

/** Fetch a resource body by URI. */
interface ReadResourceRequest extends UpstreamEnvelopeBase {
  type: "read_resource";
  /** Resource URI advertised in `discover`. */
  uri: string;
}

/** Render a prompt with user-supplied arguments. */
interface GetPromptRequest extends UpstreamEnvelopeBase {
  type: "get_prompt";
  /** Prompt id — must match one advertised in `discover`. */
  name: string;
  /** Prompt arguments, shape defined by the prompt's `arguments` list. */
  arguments?: Record<string, unknown>;
}

/** Notification that the user declined or cancelled a pending elicitation. */
interface ElicitationCancelledRequest extends UpstreamEnvelopeBase {
  type: "elicitation_cancelled";
  /** Tool id the elicitation belonged to. */
  name: string;
  /** `'decline'` or `'cancel'`. */
  action: string;
  /** Opaque value previously returned in the `elicit` reply. */
  context?: unknown;
}

/** Discriminated union of every request variant roxy may send. */
type UpstreamRequest =
  | DiscoverRequest
  | CallToolRequest
  | ReadResourceRequest
  | GetPromptRequest
  | ElicitationCancelledRequest;

// ---- Response payloads ----------------------------------------------------

/** Text content block — the most common reply kind. */
interface TextContent {
  type: "text";
  /** The string to display. */
  text: string;
}

/** Resource-link content block. Clients may follow it with `read_resource`. */
interface ResourceLinkContent {
  type: "resource_link";
  /** Target URI of the linked resource. */
  uri: string;
  /** Stable machine id. */
  name: string;
  /** Human label. */
  title?: string;
  /** Free-form description. */
  description?: string;
  /** IANA mime type of the resource body. */
  mime_type?: string;
}

type Content = TextContent | ResourceLinkContent;

/** Standard reply for tool calls, resource reads and prompts. */
interface ContentResponse {
  /** User-visible content blocks. */
  content: Content[];
  /**
   * Machine-readable result, validated against the tool's `output_schema`
   * by MCP clients that support structured output.
   */
  structured_content?: unknown;
}

/** Error reply — roxy maps this to a JSON-RPC error back to the client. */
interface ErrorResponse {
  error: {
    /** Integer status, mirrored into JSON-RPC `code`. */
    code: number;
    /** Human-readable detail. */
    message: string;
  };
}

/**
 * Elicit reply — roxy forwards it to the MCP client as an
 * `elicitation/create` request. The client asks the user and then calls the
 * same tool again with `elicitation_results` populated.
 */
interface ElicitResponse {
  elicit: {
    /** Prompt shown to the user. */
    message: string;
    /** JSON Schema describing the user's expected reply. */
    schema: Record<string, unknown>;
    /**
     * Opaque handler-side bookkeeping. roxy stores it and replays it on the
     * follow-up call so the handler can resume the flow.
     */
    context?: unknown;
  };
}

/** Any payload a handler is allowed to return. */
type HandlerResponse = ContentResponse | ErrorResponse | ElicitResponse;

// ---- Discover response ----------------------------------------------------

/** Tool advertisement returned from `discover`. */
interface ToolDef {
  /** Stable machine id — used by `call_tool`. */
  name: string;
  /** Human label shown by MCP clients. */
  title?: string;
  /** Free-form description shown by clients. */
  description?: string;
  /** JSON Schema for `arguments`. */
  input_schema?: Record<string, unknown>;
  /** Optional JSON Schema for `structured_content`. */
  output_schema?: Record<string, unknown>;
}

/** Resource advertisement returned from `discover`. */
interface ResourceDef {
  /** Identifier clients pass back in `read_resource`. */
  uri: string;
  /** Stable machine id. */
  name: string;
  /** Human label. */
  title?: string;
  /** Free-form description. */
  description?: string;
  /** IANA mime type of the resource body. */
  mime_type?: string;
}

/** Prompt argument declaration returned from `discover`. */
interface PromptArgument {
  /** Argument name. */
  name: string;
  /** Human label. */
  title?: string;
  /** Free-form description. */
  description?: string;
  /** Defaults to false. */
  required?: boolean;
}

/** Prompt advertisement returned from `discover`. */
interface PromptDef {
  /** Stable machine id — used by `get_prompt`. */
  name: string;
  /** Human label. */
  title?: string;
  /** Free-form description. */
  description?: string;
  /** Argument schema. */
  arguments?: PromptArgument[];
}

/** The full payload returned from `discover`. */
interface DiscoverResponse {
  tools: ToolDef[];
  resources: ResourceDef[];
  prompts: PromptDef[];
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/**
 * Answer a `discover` request: advertise every tool, resource and prompt this
 * backend exposes. roxy caches the result at startup and uses it to serve
 * `tools/list`, `resources/list` and `prompts/list` without re-calling us.
 */
function handleDiscover(): DiscoverResponse {
  return {
    tools: [
      {
        name: "echo", // tool id — must be unique within this backend
        title: "Echo Message", // label displayed in MCP clients
        description: "Echoes back the input message",
        input_schema: {
          type: "object", // JSON Schema root type
          properties: {
            message: {
              type: "string", // field type
              description: "The message to echo",
            },
          },
          required: ["message"], // keys that MUST be present
        },
      },
      {
        name: "add",
        title: "Add Numbers",
        description: "Adds two numbers and returns structured result",
        input_schema: {
          type: "object",
          properties: {
            // `number` covers both integer and float in JSON Schema.
            a: { type: "number", description: "First number" },
            b: { type: "number", description: "Second number" },
          },
          required: ["a", "b"],
        },
        // Optional — lets MCP clients validate `structured_content` and
        // render it with field names / types.
        output_schema: {
          type: "object",
          properties: {
            sum: { type: "number" }, // the computed sum
            operands: {
              // echoes the inputs for audit
              type: "object",
              properties: {
                a: { type: "number" },
                b: { type: "number" },
              },
            },
          },
        },
      },
      {
        name: "book_flight",
        title: "Book a Flight",
        description: "Books a flight with elicitation for missing details",
        input_schema: {
          type: "object",
          properties: {
            destination: {
              type: "string",
              description: "Flight destination",
            },
          },
          required: ["destination"],
        },
      },
    ],
    resources: [
      {
        uri: "roxy://status", // opaque, scheme is up to the backend
        name: "server-status", // stable machine id
        title: "Server Status", // shown by clients
        description: "Current server status", // shown by clients
        mime_type: "application/json", // tells the client how to render the body
      },
    ],
    prompts: [
      {
        name: "greet",
        title: "Greeting",
        description: "Generate a greeting",
        arguments: [
          {
            name: "name", // argument key passed in `get_prompt.arguments`
            title: "Person Name", // human label
            description: "Name to greet",
            required: true, // MCP clients will prompt the user before sending
          },
        ],
      },
    ],
  };
}

/** Dispatch a `call_tool` request to the matching sub-handler. */
function handleCallTool(request: CallToolRequest): HandlerResponse {
  const name = request.name;
  const args = request.arguments ?? {};
  const elicitationResults = request.elicitation_results ?? [];
  const context = request.context;

  switch (name) {
    case "echo": {
      const message = typeof args.message === "string" ? args.message : "";
      return {
        content: [
          {
            type: "text", // discriminator for the content block kind
            text: message, // the string to display
          },
        ],
      };
    }
    case "add":
      return handleAdd(args);
    case "book_flight":
      return handleBookFlight(args, elicitationResults, context);
    default:
      return {
        error: {
          code: 404,
          message: `Unknown tool: ${name}`,
        },
      };
  }
}

/** Implementation of the `add` tool. */
function handleAdd(args: Record<string, unknown>): ContentResponse {
  const a = typeof args.a === "number" ? args.a : 0;
  const b = typeof args.b === "number" ? args.b : 0;
  const sum = a + b;

  return {
    // Human-readable rendering of the result.
    content: [
      {
        type: "text",
        text: `${a} + ${b} = ${sum}`,
      },
    ],
    // Machine-readable result — matches the tool's `output_schema`.
    structured_content: {
      sum, // matches output_schema.properties.sum
      operands: { a, b }, // echoes the inputs for audit
    },
  };
}

/**
 * Multi-turn example: first call returns an `elicit` asking for flight class;
 * second call (with the elicitation reply) completes the booking.
 */
function handleBookFlight(
  args: Record<string, unknown>,
  elicitationResults: Array<Record<string, unknown>>,
  _context: unknown,
): HandlerResponse {
  const destination =
    typeof args.destination === "string" ? args.destination : "Unknown";

  // First round: no prior elicitation replies — ask the client for the class.
  if (elicitationResults.length === 0) {
    return {
      elicit: {
        message: `Select flight class for ${destination}`, // prompt shown to user
        // JSON Schema describing the expected reply. Clients render the
        // fields as a form.
        schema: {
          type: "object",
          properties: {
            class: {
              type: "string",
              title: "Flight Class",
              enum: ["economy", "business", "first"], // allowed values
            },
          },
          required: ["class"],
        },
        // Opaque handler-side bookkeeping. roxy replays it on the follow-up
        // call. Use it for whatever you need to resume — here it records
        // which step of the flow we're on.
        context: {
          destination,
          step: 1,
        },
      },
    };
  }

  // Second round: the reply carries the class the user picked.
  const reply = elicitationResults[0] ?? {};
  const cls = typeof reply.class === "string" ? reply.class : "economy";
  const bookingId = Math.floor(1000 + Math.random() * 9000);

  return {
    content: [
      // Plain-text summary.
      {
        type: "text",
        text: `Booked ${cls} flight to ${destination}. Booking #${bookingId}`,
      },
      // A `resource_link` lets the client fetch details via `read_resource`.
      {
        type: "resource_link", // discriminator
        uri: `roxy://bookings/${bookingId}`, // target of the follow-up read_resource
        name: `booking-${bookingId}`, // stable machine id
        title: `Booking #${bookingId}`, // human label
      },
    ],
    structured_content: {
      booking_id: bookingId, // generated booking id
      destination, // echoed from the original arguments
      class: cls, // value picked by the user during elicitation
    },
  };
}

/** Answer a `read_resource` request. */
function handleReadResource(request: ReadResourceRequest): HandlerResponse {
  const uri = request.uri;

  if (uri === "roxy://status") {
    return {
      // Resource bodies ride in the same `content` array as tool output.
      content: [
        {
          type: "text",
          // Stringified JSON — declared as `application/json` in discover.
          text: JSON.stringify({
            status: "running", // fixed in this demo
            node: process.version, // observed at runtime
          }),
        },
      ],
    };
  }

  return {
    error: {
      code: 404,
      message: `Unknown resource: ${uri}`,
    },
  };
}

/** Answer a `get_prompt` request. */
function handleGetPrompt(request: GetPromptRequest): HandlerResponse {
  const name = request.name;
  const args = request.arguments ?? {};

  if (name === "greet") {
    const personName = typeof args.name === "string" ? args.name : "World";
    return {
      content: [
        {
          type: "text",
          text: `Hello, ${personName}! How can I help you today?`,
        },
      ],
    };
  }

  return {
    error: {
      code: 404,
      message: `Unknown prompt: ${name}`,
    },
  };
}

/**
 * Notification that the user declined or cancelled a pending elicitation.
 * roxy doesn't wait for a meaningful body — it just wants the handler to know
 * so any server-side state tied to the flow can be cleaned up.
 */
function handleElicitationCancelled(
  request: ElicitationCancelledRequest,
): { ok: true } {
  const name = request.name; // tool the elicitation belonged to
  const action = request.action; // 'decline' or 'cancel'
  console.error(`Elicitation ${action} for tool: ${name}`);
  return { ok: true };
}

// ---------------------------------------------------------------------------
// Dispatch + HTTP server
// ---------------------------------------------------------------------------

function dispatch(request: UpstreamRequest): HandlerResponse | { ok: true } {
  switch (request.type) {
    case "discover":
      return handleDiscover() as unknown as HandlerResponse;
    case "call_tool":
      return handleCallTool(request);
    case "read_resource":
      return handleReadResource(request);
    case "get_prompt":
      return handleGetPrompt(request);
    case "elicitation_cancelled":
      return handleElicitationCancelled(request);
  }
}

async function readJsonBody(req: IncomingMessage): Promise<unknown> {
  const chunks: Buffer[] = [];
  for await (const chunk of req) {
    chunks.push(chunk as Buffer);
  }
  const body = Buffer.concat(chunks).toString("utf8");
  if (body === "") return null;
  return JSON.parse(body);
}

const server = createServer(async (req: IncomingMessage, res: ServerResponse) => {
  if (req.method !== "POST") {
    res.writeHead(405, { "Content-Type": "application/json" });
    res.end(JSON.stringify({ error: { code: 405, message: "Method not allowed" } }));
    return;
  }

  let body: unknown;
  try {
    body = await readJsonBody(req);
  } catch {
    res.writeHead(400, { "Content-Type": "application/json" });
    res.end(JSON.stringify({ error: { code: 400, message: "Invalid JSON body" } }));
    return;
  }

  if (
    body === null ||
    typeof body !== "object" ||
    !("type" in body) ||
    typeof (body as { type: unknown }).type !== "string"
  ) {
    res.writeHead(400, { "Content-Type": "application/json" });
    res.end(
      JSON.stringify({
        error: { code: 400, message: "Invalid request: missing type field" },
      }),
    );
    return;
  }

  const request = body as UpstreamRequest;

  try {
    const reply = dispatch(request);
    res.writeHead(200, { "Content-Type": "application/json" });
    res.end(JSON.stringify(reply));
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    res.writeHead(500, { "Content-Type": "application/json" });
    res.end(JSON.stringify({ error: { code: 500, message } }));
  }
});

const port = Number(process.env.PORT ?? 8000);
server.listen(port, () => {
  console.error(`roxy example upstream listening on http://127.0.0.1:${port}/`);
});
