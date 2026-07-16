<?php

namespace App\Http\Controllers;

use Illuminate\Http\Client\ConnectionException;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\Http;

class WhatsAppController extends Controller
{
    private function bridge()
    {
        $baseUrl = rtrim(config('services.whatsapp_bridge.url', env('WHATSAPP_BRIDGE_URL', 'http://127.0.0.1:4000')), '/');
        $apiKey = config('services.whatsapp_bridge.key', env('WHATSAPP_BRIDGE_KEY'));

        $pendingRequest = Http::timeout(15)->acceptJson();

        if ($apiKey) {
            $pendingRequest = $pendingRequest->withHeaders([
                'x-api-key' => $apiKey,
            ]);
        }

        return [$pendingRequest, $baseUrl];
    }

    public function status()
    {
        return $this->forward('get', '/status');
    }

    public function start()
    {
        return $this->forward('post', '/start');
    }

    public function stop()
    {
        return $this->forward('post', '/stop');
    }

    public function send(Request $request)
    {
        $validated = $request->validate([
            'to' => 'required|string|max:30',
            'message' => 'required|string|max:4000',
        ]);

        return $this->forward('post', '/send', [
            'to' => $validated['to'],
            'text' => $validated['message'],
        ]);
    }

    public function sendBulk(Request $request)
    {
        $validated = $request->validate([
            'recipients' => 'required|array|min:1|max:100',
            'recipients.*.phone' => 'required|string|max:30',
            'recipients.*.message' => 'required|string|max:4000',
        ]);

        return $this->forward('post', '/send-bulk', $validated);
    }

    private function forward(string $method, string $path, array $payload = [])
    {
        try {
            [$http, $baseUrl] = $this->bridge();
            $response = $method === 'get'
                ? $http->get($baseUrl.$path)
                : $http->post($baseUrl.$path, $payload);

            return response()->json(
                $response->json() ?? ['message' => $response->body()],
                $response->status()
            );
        } catch (ConnectionException $exception) {
            return response()->json([
                'message' => 'WhatsApp bridge belum berjalan',
                'error' => $exception->getMessage(),
            ], 503);
        }
    }
}
