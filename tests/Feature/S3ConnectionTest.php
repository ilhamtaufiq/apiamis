<?php

namespace Tests\Feature;

use Tests\TestCase;

class S3ConnectionTest extends TestCase
{
    protected function setUp(): void
    {
        parent::setUp();
        $this->withoutMiddleware();
    }

    public function test_s3_connection_validation_fails_without_credentials()
    {
        $response = $this->postJson('/api/app-settings/backups/s3/test', [
            's3_endpoint' => '',
            's3_region' => '',
            's3_bucket' => '',
            's3_access_key_id' => '',
            's3_secret_access_key' => '',
        ]);

        $response->assertStatus(400)
            ->assertJson([
                'ok' => false,
            ]);
    }

    public function test_s3_connection_validation_attempts_and_catches_invalid_connection()
    {
        $response = $this->postJson('/api/app-settings/backups/s3/test', [
            's3_endpoint' => 'https://s3.us-east-1.amazonaws.com',
            's3_region' => 'us-east-1',
            's3_bucket' => 'invalid-bucket-name-for-testing-purposes-123',
            's3_access_key_id' => 'mock-access-key',
            's3_secret_access_key' => 'mock-secret-key',
        ]);

        $response->assertStatus(422)
            ->assertJson([
                'ok' => false,
            ]);

        $this->assertStringContainsString('Koneksi S3 gagal', $response->json('error'));
    }
}
