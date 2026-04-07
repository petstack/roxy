<?php

declare(strict_types=1);

$input = file_get_contents('php://input');
if ($input === '' || $input === false) {
    $input = file_get_contents('php://stdin');
}
$request = json_decode($input, true);

header('Content-Type: application/json');

if (!$request || !isset($request['type'])) {
    echo json_encode([
        'error' => ['code' => 400, 'message' => 'Invalid request: missing type field'],
    ]);
    exit;
}

// Envelope fields available on all requests
$sessionId = $request['session_id'] ?? null;
$requestId = $request['request_id'] ?? null;

echo match ($request['type']) {
    'discover' => handleDiscover(),
    'call_tool' => handleCallTool($request),
    'read_resource' => handleReadResource($request),
    'get_prompt' => handleGetPrompt($request),
    'elicitation_cancelled' => handleElicitationCancelled($request),
    default => json_encode([
        'error' => ['code' => 400, 'message' => "Unknown request type: {$request['type']}"],
    ]),
};

function handleDiscover(): string
{
    return json_encode([
        'tools' => [
            [
                'name' => 'echo',
                'title' => 'Echo Message',
                'description' => 'Echoes back the input message',
                'input_schema' => [
                    'type' => 'object',
                    'properties' => [
                        'message' => [
                            'type' => 'string',
                            'description' => 'The message to echo',
                        ],
                    ],
                    'required' => ['message'],
                ],
            ],
            [
                'name' => 'add',
                'title' => 'Add Numbers',
                'description' => 'Adds two numbers and returns structured result',
                'input_schema' => [
                    'type' => 'object',
                    'properties' => [
                        'a' => ['type' => 'number', 'description' => 'First number'],
                        'b' => ['type' => 'number', 'description' => 'Second number'],
                    ],
                    'required' => ['a', 'b'],
                ],
                'output_schema' => [
                    'type' => 'object',
                    'properties' => [
                        'sum' => ['type' => 'number'],
                        'operands' => [
                            'type' => 'object',
                            'properties' => [
                                'a' => ['type' => 'number'],
                                'b' => ['type' => 'number'],
                            ],
                        ],
                    ],
                ],
            ],
            [
                'name' => 'book_flight',
                'title' => 'Book a Flight',
                'description' => 'Books a flight with elicitation for missing details',
                'input_schema' => [
                    'type' => 'object',
                    'properties' => [
                        'destination' => [
                            'type' => 'string',
                            'description' => 'Flight destination',
                        ],
                    ],
                    'required' => ['destination'],
                ],
            ],
        ],
        'resources' => [
            [
                'uri' => 'roxy://status',
                'name' => 'server-status',
                'title' => 'Server Status',
                'description' => 'Current server status',
                'mime_type' => 'application/json',
            ],
        ],
        'prompts' => [
            [
                'name' => 'greet',
                'title' => 'Greeting',
                'description' => 'Generate a greeting',
                'arguments' => [
                    ['name' => 'name', 'title' => 'Person Name', 'description' => 'Name to greet', 'required' => true],
                ],
            ],
        ],
    ]);
}

function handleCallTool(array $request): string
{
    $name = $request['name'] ?? '';
    $args = $request['arguments'] ?? [];
    $elicitationResults = $request['elicitation_results'] ?? [];
    $context = $request['context'] ?? null;

    return match ($name) {
        'echo' => json_encode([
            'content' => [['type' => 'text', 'text' => $args['message'] ?? '']],
        ]),
        'add' => handleAdd($args),
        'book_flight' => handleBookFlight($args, $elicitationResults, $context),
        default => json_encode([
            'error' => ['code' => 404, 'message' => "Unknown tool: {$name}"],
        ]),
    };
}

function handleAdd(array $args): string
{
    $a = $args['a'] ?? 0;
    $b = $args['b'] ?? 0;
    $sum = $a + $b;

    return json_encode([
        'content' => [['type' => 'text', 'text' => "{$a} + {$b} = {$sum}"]],
        'structured_content' => [
            'sum' => $sum,
            'operands' => ['a' => $a, 'b' => $b],
        ],
    ]);
}

function handleBookFlight(array $args, array $elicitationResults, mixed $context): string
{
    $destination = $args['destination'] ?? 'Unknown';

    // First round: no elicitation results yet — ask for flight class
    if (empty($elicitationResults)) {
        return json_encode([
            'elicit' => [
                'message' => "Select flight class for {$destination}",
                'schema' => [
                    'type' => 'object',
                    'properties' => [
                        'class' => [
                            'type' => 'string',
                            'title' => 'Flight Class',
                            'enum' => ['economy', 'business', 'first'],
                        ],
                    ],
                    'required' => ['class'],
                ],
                'context' => ['destination' => $destination, 'step' => 1],
            ],
        ]);
    }

    // Second round: we have the class
    $class = $elicitationResults[0]['class'] ?? 'economy';
    $bookingId = rand(1000, 9999);

    return json_encode([
        'content' => [
            ['type' => 'text', 'text' => "Booked {$class} flight to {$destination}. Booking #{$bookingId}"],
            ['type' => 'resource_link', 'uri' => "roxy://bookings/{$bookingId}", 'name' => "booking-{$bookingId}", 'title' => "Booking #{$bookingId}"],
        ],
        'structured_content' => [
            'booking_id' => $bookingId,
            'destination' => $destination,
            'class' => $class,
        ],
    ]);
}

function handleReadResource(array $request): string
{
    $uri = $request['uri'] ?? '';

    if ($uri === 'roxy://status') {
        return json_encode([
            'content' => [['type' => 'text', 'text' => json_encode(['status' => 'running', 'php' => PHP_VERSION])]],
        ]);
    }

    return json_encode([
        'error' => ['code' => 404, 'message' => "Unknown resource: {$uri}"],
    ]);
}

function handleGetPrompt(array $request): string
{
    $name = $request['name'] ?? '';
    $args = $request['arguments'] ?? [];

    if ($name === 'greet') {
        $personName = $args['name'] ?? 'World';
        return json_encode([
            'content' => [['type' => 'text', 'text' => "Hello, {$personName}! How can I help you today?"]],
        ]);
    }

    return json_encode([
        'error' => ['code' => 404, 'message' => "Unknown prompt: {$name}"],
    ]);
}

function handleElicitationCancelled(array $request): string
{
    // Log the cancellation; no meaningful response needed
    $name = $request['name'] ?? '';
    $action = $request['action'] ?? '';
    error_log("Elicitation {$action} for tool: {$name}");

    return json_encode(['ok' => true]);
}
