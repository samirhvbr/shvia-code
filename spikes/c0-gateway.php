<?php
/**
 * C0 — Spike do gateway (SHVIA-CODE).
 *
 * Prova o LOOP AGÊNTICO completo contra POST /api/v1/code/chat (SHVIA-WEB
 * 2.16.0): pergunta → tool_call → execução LOCAL → role=tool → resposta final.
 * É o embrião do loop do CLI `anna` (que será Rust): mesmo protocolo, mesmos
 * gates conceituais. PHP puro (curl) pra rodar em qualquer máquina da casa.
 *
 * USO:
 *   SHVIA_API_KEY=sk-...  php spikes/c0-gateway.php
 *   # opcionais:
 *   SHVIA_URL=https://ia.blue3.com.br     # default
 *   SHVIA_PROFILE="model@server"          # perfil explícito (senão: default do usuário)
 *   SHVIA_PROMPT="..."                    # pergunta customizada
 *
 * DoD do C0: transcript com o loop fechado (tool_call → resultado → resposta),
 * colado em .continue/estado-atual.md — rodar com um modelo on-prem E um de
 * nuvem pra medir a qualidade de tool-calling de cada um.
 *
 * Segurança (SECURITY_GUIDELINES §2): as tools deste spike são read-only e
 * CONFINADAS a um sandbox criado em /tmp — nem o modelo escolhendo paths
 * maliciosos escapa (realpath + prefixo). Denylist de .env/chaves ativa.
 */

declare(strict_types=1);

error_reporting(E_ALL);
ini_set('display_errors', '1');

// ── Config ───────────────────────────────────────────────────────────────────
$apiKey = getenv('SHVIA_API_KEY') ?: '';
if ($apiKey === '') {
    fwrite(STDERR, "ERRO: exporte SHVIA_API_KEY (painel do ShvIA → Conta → API).\n");
    exit(2);
}
$baseUrl = rtrim(getenv('SHVIA_URL') ?: 'https://ia.blue3.com.br', '/');
$profile = getenv('SHVIA_PROFILE') ?: null;
$maxLoops = 6;

// ── Sandbox: mini-projeto de exemplo em /tmp (as tools só enxergam ele) ──────
$sandbox = sys_get_temp_dir().'/shvia-code-c0';
@mkdir($sandbox, 0700, true);
file_put_contents($sandbox.'/README.md', "# Projeto Demo\nServiço de boletos da Blue3 (exemplo do spike C0).\n");
file_put_contents($sandbox.'/config.php', "<?php\nreturn [\n    'timezone' => 'America/Sao_Paulo',\n    'retries'  => 3,\n    // BUG proposital: deveria ser 'https'\n    'scheme'   => 'htps',\n];\n");
file_put_contents($sandbox.'/.env', "SEGREDO=nao-deveria-ser-lido\n");

// ── Tools locais (read-only, confinadas ao sandbox) ─────────────────────────
$toolDefs = [
    [
        'name'        => 'list_dir',
        'description' => 'Lista os arquivos e pastas de um diretório do projeto.',
        'parameters'  => [
            'type'       => 'object',
            'properties' => ['path' => ['type' => 'string', 'description' => 'Caminho relativo à raiz do projeto (use "." para a raiz).']],
            'required'   => ['path'],
        ],
    ],
    [
        'name'        => 'read_file',
        'description' => 'Lê o conteúdo de um arquivo do projeto.',
        'parameters'  => [
            'type'       => 'object',
            'properties' => ['path' => ['type' => 'string', 'description' => 'Caminho relativo à raiz do projeto.']],
            'required'   => ['path'],
        ],
    ],
];

/** Resolve um path RELATIVO dentro do sandbox; null se escapar ou não existir. */
function sandbox_path(string $sandbox, string $rel): ?string
{
    $full = realpath($sandbox.'/'.ltrim($rel === '.' ? '' : $rel, '/'));
    if ($full === false || ! str_starts_with($full, realpath($sandbox) ?: "\0")) {
        return null;
    }

    return $full;
}

/** Denylist do SECURITY_GUIDELINES §2.3 — vale até no spike. */
function denied(string $rel): bool
{
    $base = basename($rel);

    return $base === '.env' || str_starts_with($base, '.env.')
        || preg_match('/\.(pem|key|p8|p12|pfx)$/i', $base) === 1
        || str_contains($rel, 'id_rsa') || str_contains($rel, 'credentials');
}

function run_tool(string $sandbox, string $name, array $args): string
{
    $rel = (string) ($args['path'] ?? '.');
    if (denied($rel)) {
        return "RECUSADO: '$rel' está na denylist de segredos (SECURITY_GUIDELINES §2.3).";
    }
    $full = sandbox_path($sandbox, $rel);
    if ($full === null) {
        return "ERRO: caminho '$rel' inexistente ou fora do projeto.";
    }

    return match ($name) {
        'list_dir'  => is_dir($full)
            ? implode("\n", array_values(array_diff(scandir($full) ?: [], ['.', '..'])))
            : "ERRO: '$rel' não é diretório.",
        'read_file' => is_file($full)
            ? (string) file_get_contents($full)
            : "ERRO: '$rel' não é arquivo.",
        default     => "ERRO: tool desconhecida '$name'.",
    };
}

// ── Cliente SSE ──────────────────────────────────────────────────────────────
/**
 * POST SSE: devolve ['text'=>..., 'tool_calls'=>[...], 'usage'=>[...], 'model'=>[...]]
 * (agregado dos eventos), imprimindo os deltas ao vivo.
 */
function code_chat(string $baseUrl, string $apiKey, array $payload): array
{
    $out = ['text' => '', 'tool_calls' => [], 'usage' => null, 'model' => null, 'error' => null];
    $buffer = '';

    $ch = curl_init($baseUrl.'/api/v1/code/chat');
    curl_setopt_array($ch, [
        CURLOPT_POST           => true,
        CURLOPT_POSTFIELDS     => json_encode($payload, JSON_UNESCAPED_UNICODE),
        CURLOPT_HTTPHEADER     => [
            'Content-Type: application/json',
            'Accept: text/event-stream',
            'X-API-Key: '.$apiKey,
        ],
        CURLOPT_WRITEFUNCTION  => function ($ch, string $chunk) use (&$buffer, &$out): int {
            $buffer .= $chunk;
            while (($pos = strpos($buffer, "\n\n")) !== false) {
                $frame = substr($buffer, 0, $pos);
                $buffer = substr($buffer, $pos + 2);
                foreach (explode("\n", $frame) as $line) {
                    if (! str_starts_with($line, 'data: ')) {
                        continue;
                    }
                    $evt = json_decode(substr($line, 6), true);
                    if (! is_array($evt)) {
                        continue;
                    }
                    $kind = $evt['kind'] ?? '';
                    $data = $evt['data'] ?? [];
                    switch ($kind) {
                        case 'model':
                            $out['model'] = $data;
                            printf("  [modelo] %s @ %s\n", $data['model'] ?? '?', $data['server_label'] ?? $data['server'] ?? '?');
                            break;
                        case 'text_delta':
                            $out['text'] .= (string) ($data['delta'] ?? '');
                            echo $data['delta'] ?? '';
                            break;
                        case 'reasoning_delta':
                            break; // pensamento: ignorado no transcript do spike
                        case 'tool_call':
                            $out['tool_calls'][] = $data;
                            printf("\n  [tool_call] %s(%s)\n", $data['name'] ?? '?', json_encode($data['arguments'] ?? [], JSON_UNESCAPED_UNICODE));
                            break;
                        case 'usage':
                            $out['usage'] = $data;
                            break;
                        case 'error':
                            $out['error'] = (string) ($data['message'] ?? 'erro');
                            printf("\n  [ERRO servidor] %s\n", $out['error']);
                            break;
                        case 'done':
                            if (! empty($data['tool_calls']) && $out['tool_calls'] === []) {
                                $out['tool_calls'] = $data['tool_calls']; // fallback: cliente que não pegou no stream
                            }
                            break;
                    }
                }
            }

            return strlen($chunk);
        },
        CURLOPT_TIMEOUT        => 300,
        CURLOPT_FAILONERROR    => false,
    ]);
    curl_exec($ch);
    $status = (int) curl_getinfo($ch, CURLINFO_RESPONSE_CODE);
    $err = curl_error($ch);
    curl_close($ch);

    if ($err !== '') {
        $out['error'] = $out['error'] ?? "curl: $err";
    } elseif ($status >= 400 && $out['error'] === null) {
        $out['error'] = "HTTP $status";
    }

    return $out;
}

// ── O loop agêntico (o coração do futuro `anna`) ─────────────────────────────
$prompt = getenv('SHVIA_PROMPT')
    ?: 'Explore o projeto: liste os arquivos da raiz, leia o config.php e me diga se há algum problema na configuração.';

$messages = [
    ['role' => 'system', 'content' => 'Você é a Anna, agente de código da Blue3. Use as ferramentas disponíveis para inspecionar o projeto antes de responder. Responda em pt-BR, direto ao ponto.'],
    ['role' => 'user', 'content' => $prompt],
];

printf("╭─ C0 · spike do gateway — %s\n", $baseUrl);
printf("│  pergunta: %s\n", $prompt);
printf("╰─ sandbox: %s\n\n", $sandbox);

$totalTokens = 0;
$totalCost = 0.0;
$costIsEstimate = false;

for ($round = 1; $round <= $maxLoops; $round++) {
    printf("── volta %d ──────────────────────────────────\n", $round);
    $payload = [
        'messages' => $messages,
        'tools'    => $toolDefs,
    ];
    if ($profile !== null && $profile !== '') {
        $payload['profile_name'] = $profile;
    }

    $res = code_chat($baseUrl, $apiKey, $payload);
    echo "\n";

    if ($res['error'] !== null) {
        fwrite(STDERR, "ABORTADO: {$res['error']}\n");
        exit(1);
    }
    if ($res['usage'] !== null) {
        $totalTokens += (int) ($res['usage']['total_tokens'] ?? 0);
        if (isset($res['usage']['cost'])) {
            $totalCost += (float) $res['usage']['cost'];
        } elseif (isset($res['usage']['cost_estimated'])) {
            $totalCost += (float) $res['usage']['cost_estimated'];
            $costIsEstimate = true;
        }
    }

    if ($res['tool_calls'] === []) {
        // Sem tool calls → resposta final.
        printf("\n╭─ RESULTADO (loop fechado em %d volta(s))\n", $round);
        printf("│  tokens: %s · custo: %s%s\n", number_format($totalTokens, 0, ',', '.'),
            $costIsEstimate ? '≈' : '', 'US$ '.number_format($totalCost, 6));
        echo "╰─ C0 PROVADO: pergunta → tool_call → resultado → resposta.\n";
        exit(0);
    }

    // Executa as tools LOCALMENTE e devolve os resultados (role=tool).
    $messages[] = [
        'role'       => 'assistant',
        'content'    => $res['text'],
        'tool_calls' => $res['tool_calls'],
    ];
    foreach ($res['tool_calls'] as $call) {
        $result = run_tool($sandbox, (string) ($call['name'] ?? ''), (array) ($call['arguments'] ?? []));
        printf("  [executei] %s → %d bytes\n", $call['name'] ?? '?', strlen($result));
        $messages[] = [
            'role'         => 'tool',
            'tool_call_id' => (string) ($call['id'] ?? ''),
            'content'      => $result,
        ];
    }
    echo "\n";
}

fwrite(STDERR, "ABORTADO: excedeu $maxLoops voltas sem resposta final (teto do SECURITY §2.7).\n");
exit(1);
