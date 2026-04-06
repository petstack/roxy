<?php

declare(strict_types=1);

// In FastCGI mode, the request body is available via php://input.
// For CLI testing, fall back to php://stdin so you can pipe JSON directly.
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

echo match ($request['type']) {
    'discover' => handleDiscover(),
    'call_tool' => handleCallTool($request),
    'read_resource' => handleReadResource($request),
    'get_prompt' => handleGetPrompt($request),
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
                'description' => 'Adds two numbers',
                'input_schema' => [
                    'type' => 'object',
                    'properties' => [
                        'a' => ['type' => 'number', 'description' => 'First number'],
                        'b' => ['type' => 'number', 'description' => 'Second number'],
                    ],
                    'required' => ['a', 'b'],
                ],
            ],
        ],
        'resources' => [
            [
                'uri' => 'roxy://status',
                'name' => 'server-status',
                'description' => 'Current server status',
                'mime_type' => 'application/json',
            ],
        ],
        'prompts' => [
            [
                'name' => 'greet',
                'description' => 'Generate a greeting',
                'arguments' => [
                    ['name' => 'name', 'description' => 'Name to greet', 'required' => true],
                ],
            ],
        ],
    ]);
}

function handleCallTool(array $request): string
{
    $name = $request['name'] ?? '';
    $args = $request['arguments'] ?? [];

    return match ($name) {
        'echo' => json_encode([
            'content' => [['type' => 'text', 'text' => $args['message'] ?? '']],
        ]),
        'add' => json_encode([
            'content' => [['type' => 'text', 'text' => (string)(($args['a'] ?? 0) + ($args['b'] ?? 0))]],
        ]),
        default => json_encode([
            'error' => ['code' => 404, 'message' => "Unknown tool: {$name}"],
        ]),
    };
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
