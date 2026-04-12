<?php

declare(strict_types=1);

/**
 * Example roxy upstream handler (FastCGI / HTTP).
 *
 * roxy translates MCP JSON-RPC into a simple envelope and POSTs it to this
 * script. The handler inspects `type` and returns a JSON payload that roxy
 * turns back into an MCP response.
 *
 * All array shapes below use PHPStan / Psalm syntax (`array{key: T, ...}`) so
 * static analyzers can verify both the decoded request and the encoded reply.
 *
 * See `src/protocol.rs` (`UpstreamEnvelope`, `UpstreamRequest`,
 * `UpstreamCallResult`) for the authoritative Rust-side contract.
 */

// Prefer the CGI/FastCGI body stream (`php://input`); fall back to STDIN so the
// script can also be driven from the shell for local debugging.
$input = file_get_contents('php://input');
if ($input === '' || $input === false) {
    $input = file_get_contents('php://stdin');
}

/**
 * Decoded envelope sent by roxy. Every request carries the common envelope
 * fields (`session_id`, `request_id`, `type`) plus fields specific to the
 * discriminated `type` variant.
 *
 * @var array{
 *     type?: 'discover'|'call_tool'|'read_resource'|'get_prompt'|'elicitation_cancelled',
 *     session_id?: string|null,  // MCP session id, null under stdio transport
 *     request_id?: string,       // opaque id used by roxy to correlate replies
 *     name?: string,             // tool / prompt name (call_tool, get_prompt, elicitation_cancelled)
 *     arguments?: array<string, mixed>,   // tool / prompt arguments
 *     elicitation_results?: list<array<string, mixed>>, // prior elicitation replies, oldest first
 *     context?: mixed,           // opaque value previously returned in an `elicit` reply
 *     uri?: string,              // resource URI (read_resource)
 *     action?: string,           // elicitation action: 'decline' | 'cancel'
 * }|null|false $request
 */
$request = json_decode($input, true);

header('Content-Type: application/json');

if (!$request || !isset($request['type'])) {
    echo json_encode([
        // Error envelope — roxy maps this to a JSON-RPC error back to the client.
        'error' => [
            'code' => 400,               // integer status, mirrored into JSON-RPC `code`
            'message' => 'Invalid request: missing type field', // human-readable detail
        ],
    ]);
    exit;
}

// Envelope fields present on every request. Kept around for logging / audit;
// the example handlers below don't use them but real deployments typically do.
$sessionId = $request['session_id'] ?? null;
$requestId = $request['request_id'] ?? null;

echo match ($request['type']) {
    'discover'              => handleDiscover(),
    'call_tool'             => handleCallTool($request),
    'read_resource'         => handleReadResource($request),
    'get_prompt'            => handleGetPrompt($request),
    'elicitation_cancelled' => handleElicitationCancelled($request),
    default => json_encode([
        'error' => [
            'code' => 400,
            'message' => "Unknown request type: {$request['type']}",
        ],
    ]),
};

/**
 * Answer a `discover` request: advertise every tool, resource and prompt this
 * backend exposes. roxy caches the result at startup and uses it to serve
 * `tools/list`, `resources/list` and `prompts/list` MCP methods without
 * calling the backend again.
 *
 * @return string JSON-encoded {@see UpstreamDiscoverResponse} payload.
 */
function handleDiscover(): string
{
    return json_encode([
        /**
         * Tool catalog.
         *
         * @var list<array{
         *     name: string,               // stable machine id — used by call_tool
         *     title?: string,              // human label shown by MCP clients
         *     description?: string,        // free-form description shown by clients
         *     input_schema?: array<string, mixed>,  // JSON Schema for `arguments`
         *     output_schema?: array<string, mixed>, // optional JSON Schema for `structured_content`
         * }>
         */
        'tools' => [
            [
                'name'        => 'echo',                  // tool id — must be unique within this backend
                'title'       => 'Echo Message',          // label displayed in MCP clients
                'description' => 'Echoes back the input message', // shown next to the title
                'input_schema' => [
                    'type'       => 'object',             // JSON Schema root type
                    'properties' => [
                        'message' => [
                            'type'        => 'string',    // JSON Schema type for this field
                            'description' => 'The message to echo', // field-level hint
                        ],
                    ],
                    'required' => ['message'],            // list of keys that MUST be present
                ],
            ],
            [
                'name'        => 'add',
                'title'       => 'Add Numbers',
                'description' => 'Adds two numbers and returns structured result',
                'input_schema' => [
                    'type'       => 'object',
                    'properties' => [
                        // `number` covers both integer and float in JSON Schema.
                        'a' => ['type' => 'number', 'description' => 'First number'],
                        'b' => ['type' => 'number', 'description' => 'Second number'],
                    ],
                    'required' => ['a', 'b'],
                ],
                // Optional — when set, MCP clients can validate `structured_content`
                // and render it with field names / types.
                'output_schema' => [
                    'type'       => 'object',
                    'properties' => [
                        'sum'      => ['type' => 'number'], // the computed sum
                        'operands' => [                     // echoes the inputs for audit
                            'type'       => 'object',
                            'properties' => [
                                'a' => ['type' => 'number'],
                                'b' => ['type' => 'number'],
                            ],
                        ],
                    ],
                ],
            ],
            [
                'name'        => 'book_flight',
                'title'       => 'Book a Flight',
                'description' => 'Books a flight with elicitation for missing details',
                'input_schema' => [
                    'type'       => 'object',
                    'properties' => [
                        'destination' => [
                            'type'        => 'string',
                            'description' => 'Flight destination',
                        ],
                    ],
                    'required' => ['destination'],
                ],
            ],
        ],
        /**
         * Resource catalog.
         *
         * @var list<array{
         *     uri: string,           // identifier clients pass back in `read_resource`
         *     name: string,          // stable machine id
         *     title?: string,        // human label
         *     description?: string,  // free-form description
         *     mime_type?: string,    // IANA mime type of the resource body
         * }>
         */
        'resources' => [
            [
                'uri'         => 'roxy://status',        // opaque, scheme is up to the backend
                'name'        => 'server-status',         // stable machine id
                'title'       => 'Server Status',         // shown by clients
                'description' => 'Current server status', // shown by clients
                'mime_type'   => 'application/json',      // tells the client how to render the body
            ],
        ],
        /**
         * Prompt catalog.
         *
         * @var list<array{
         *     name: string,          // stable machine id — used by get_prompt
         *     title?: string,        // human label
         *     description?: string,  // free-form description
         *     arguments?: list<array{
         *         name: string,          // argument name
         *         title?: string,        // human label
         *         description?: string,  // free-form description
         *         required?: bool,       // defaults to false
         *     }>,
         * }>
         */
        'prompts' => [
            [
                'name'        => 'greet',
                'title'       => 'Greeting',
                'description' => 'Generate a greeting',
                'arguments' => [
                    [
                        'name'        => 'name',         // argument key passed in `get_prompt.arguments`
                        'title'       => 'Person Name',  // human label
                        'description' => 'Name to greet',
                        'required'    => true,           // MCP clients will prompt the user before sending
                    ],
                ],
            ],
        ],
    ]);
}

/**
 * Dispatch a `call_tool` request to the matching handler.
 *
 * @param array{
 *     name?: string,
 *     arguments?: array<string, mixed>,
 *     elicitation_results?: list<array<string, mixed>>,
 *     context?: mixed,
 * } $request Decoded envelope fields for the `call_tool` variant.
 * @return string JSON-encoded {@see UpstreamCallResult} payload.
 */
function handleCallTool(array $request): string
{
    $name                = $request['name']                ?? '';
    $args                = $request['arguments']           ?? [];
    // Ordered list of previous elicitation replies from the client, oldest first.
    // Empty on the first call; populated on each follow-up while a multi-step
    // elicitation flow is in progress.
    $elicitationResults  = $request['elicitation_results'] ?? [];
    // Opaque value that this handler returned inside a previous `elicit` reply.
    // roxy threads it back verbatim so the handler can restore its own state.
    $context             = $request['context']             ?? null;

    return match ($name) {
        'echo' => json_encode([
            /**
             * Content blocks are the user-visible part of the reply.
             *
             * @var list<array{type: 'text', text: string}|array{
             *     type: 'resource_link',
             *     uri: string,
             *     name: string,
             *     title?: string,
             *     description?: string,
             *     mime_type?: string,
             * }>
             */
            'content' => [
                [
                    'type' => 'text',                         // discriminator for the content block kind
                    'text' => $args['message'] ?? '',         // the string to display
                ],
            ],
        ]),
        'add'         => handleAdd($args),
        'book_flight' => handleBookFlight($args, $elicitationResults, $context),
        default       => json_encode([
            'error' => [
                'code'    => 404,
                'message' => "Unknown tool: {$name}",
            ],
        ]),
    };
}

/**
 * Implementation of the `add` tool.
 *
 * @param array{a?: int|float, b?: int|float} $args Tool arguments as declared by `input_schema`.
 * @return string JSON-encoded content + `structured_content` reply.
 */
function handleAdd(array $args): string
{
    $a   = $args['a'] ?? 0;
    $b   = $args['b'] ?? 0;
    $sum = $a + $b;

    return json_encode([
        // Human-readable rendering of the result.
        'content' => [
            [
                'type' => 'text',
                'text' => "{$a} + {$b} = {$sum}",
            ],
        ],
        /**
         * Machine-readable result, validated against the tool's `output_schema`
         * by MCP clients that support structured output.
         *
         * @var array{sum: int|float, operands: array{a: int|float, b: int|float}}
         */
        'structured_content' => [
            'sum'      => $sum,                       // matches output_schema.properties.sum
            'operands' => ['a' => $a, 'b' => $b],     // echoes the inputs for audit
        ],
    ]);
}

/**
 * Multi-turn example: first call returns an `elicit` asking for flight class;
 * second call (with the elicitation reply) completes the booking.
 *
 * @param array{destination?: string}                  $args               Tool arguments.
 * @param list<array<string, mixed>>                   $elicitationResults Prior user replies.
 * @param mixed                                         $context            Opaque value returned in the previous `elicit`.
 * @return string JSON-encoded {@see UpstreamCallResult} payload.
 */
function handleBookFlight(array $args, array $elicitationResults, mixed $context): string
{
    $destination = $args['destination'] ?? 'Unknown';

    // First round: no prior elicitation replies — ask the client for the class.
    if (empty($elicitationResults)) {
        return json_encode([
            /**
             * Elicit reply — roxy forwards it to the MCP client as an
             * `elicitation/create` request. The client asks the user and then
             * calls this same tool again with `elicitation_results` populated.
             *
             * @var array{
             *     message: string,
             *     schema: array<string, mixed>,
             *     context?: mixed,
             * }
             */
            'elicit' => [
                'message' => "Select flight class for {$destination}", // prompt shown to the user
                // JSON Schema describing the user's expected reply. Clients
                // render the fields as a form.
                'schema' => [
                    'type'       => 'object',
                    'properties' => [
                        'class' => [
                            'type'  => 'string',
                            'title' => 'Flight Class',
                            'enum'  => ['economy', 'business', 'first'], // allowed values
                        ],
                    ],
                    'required' => ['class'],
                ],
                // Opaque handler-side bookkeeping. roxy stores it and replays
                // it to us on the follow-up call. Use it for whatever you need
                // to resume — here it records which step of the flow we're on.
                'context' => [
                    'destination' => $destination,
                    'step'        => 1,
                ],
            ],
        ]);
    }

    // Second round: the reply carries the class the user picked.
    $class     = $elicitationResults[0]['class'] ?? 'economy';
    $bookingId = rand(1000, 9999);

    return json_encode([
        'content' => [
            // Plain-text summary.
            [
                'type' => 'text',
                'text' => "Booked {$class} flight to {$destination}. Booking #{$bookingId}",
            ],
            // A `resource_link` lets the client fetch details via `read_resource`.
            [
                'type'  => 'resource_link',                         // discriminator
                'uri'   => "roxy://bookings/{$bookingId}",          // target of the follow-up read_resource
                'name'  => "booking-{$bookingId}",                  // stable machine id
                'title' => "Booking #{$bookingId}",                 // human label
            ],
        ],
        /**
         * Structured equivalent of the booking for programmatic consumers.
         *
         * @var array{booking_id: int, destination: string, class: string}
         */
        'structured_content' => [
            'booking_id'  => $bookingId,   // generated booking id
            'destination' => $destination, // echoed from the original arguments
            'class'       => $class,       // value picked by the user during elicitation
        ],
    ]);
}

/**
 * Answer a `read_resource` request.
 *
 * @param array{uri?: string} $request Decoded envelope for the read_resource variant.
 * @return string JSON-encoded content reply or error payload.
 */
function handleReadResource(array $request): string
{
    $uri = $request['uri'] ?? '';

    if ($uri === 'roxy://status') {
        return json_encode([
            // Resource bodies ride in the same `content` array as tool output.
            'content' => [
                [
                    'type' => 'text',
                    // Stringified JSON — declared as `application/json` in discover.
                    'text' => json_encode([
                        'status' => 'running',    // fixed in this demo
                        'php'    => PHP_VERSION,  // observed at runtime
                    ]),
                ],
            ],
        ]);
    }

    return json_encode([
        'error' => [
            'code'    => 404,
            'message' => "Unknown resource: {$uri}",
        ],
    ]);
}

/**
 * Answer a `get_prompt` request.
 *
 * @param array{name?: string, arguments?: array<string, mixed>} $request
 * @return string JSON-encoded content reply or error payload.
 */
function handleGetPrompt(array $request): string
{
    $name = $request['name']      ?? '';
    $args = $request['arguments'] ?? [];

    if ($name === 'greet') {
        $personName = $args['name'] ?? 'World';
        return json_encode([
            'content' => [
                [
                    'type' => 'text',
                    'text' => "Hello, {$personName}! How can I help you today?",
                ],
            ],
        ]);
    }

    return json_encode([
        'error' => [
            'code'    => 404,
            'message' => "Unknown prompt: {$name}",
        ],
    ]);
}

/**
 * Notification that the user declined or cancelled a pending elicitation.
 * roxy doesn't wait for a meaningful body — it just wants the handler to know
 * so any server-side state tied to the flow can be cleaned up.
 *
 * @param array{name?: string, action?: string, context?: mixed} $request
 * @return string JSON-encoded acknowledgement `{ok: true}`.
 */
function handleElicitationCancelled(array $request): string
{
    $name   = $request['name']   ?? '';  // tool the elicitation belonged to
    $action = $request['action'] ?? '';  // 'decline' or 'cancel'
    error_log("Elicitation {$action} for tool: {$name}");

    return json_encode(['ok' => true]);
}
