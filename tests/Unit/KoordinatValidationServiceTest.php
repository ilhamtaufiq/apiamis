<?php

namespace Tests\Unit;

use App\Services\KoordinatValidationService;
use PHPUnit\Framework\Attributes\Test;
use Tests\TestCase;

class KoordinatValidationServiceTest extends TestCase
{
    private KoordinatValidationService $service;

    protected function setUp(): void
    {
        parent::setUp();
        $this->service = new KoordinatValidationService;
    }

    #[Test]
    public function it_parses_coordinates_with_comma_separator(): void
    {
        $parsed = $this->service->parseKoordinat('-7.165398, 107.154517');

        $this->assertNotNull($parsed);
        $this->assertEqualsWithDelta(-7.165398, $parsed['lat'], 0.000001);
        $this->assertEqualsWithDelta(107.154517, $parsed['lng'], 0.000001);
    }

    #[Test]
    public function it_parses_coordinates_without_comma_separator(): void
    {
        $parsed = $this->service->parseKoordinat('-7.1653984107.1545166');

        $this->assertNotNull($parsed);
        $this->assertEqualsWithDelta(-7.16539841, $parsed['lat'], 0.000001);
        $this->assertEqualsWithDelta(107.1545166, $parsed['lng'], 0.000001);
    }

    #[Test]
    public function it_rejects_manual_or_invalid_coordinates(): void
    {
        $this->assertNull($this->service->parseKoordinat('manual'));
        $this->assertNull($this->service->parseKoordinat('not-a-coordinate'));
    }

    #[Test]
    public function it_detects_point_inside_babakancaringin_polygon(): void
    {
        $reflection = new \ReflectionClass($this->service);
        $loadIndex = $reflection->getMethod('loadVillageIndex');
        $loadIndex->setAccessible(true);
        $index = $loadIndex->invoke($this->service);

        $feature = $index['karangtengah|babakancaringin'] ?? null;
        $this->assertNotNull($feature, 'GeoJSON feature for Babakancaringin should exist');

        $pointInside = $reflection->getMethod('pointInsideFeature');
        $pointInside->setAccessible(true);

        $this->assertTrue($pointInside->invoke($this->service, 107.21, -6.8, $feature));
        $this->assertFalse($pointInside->invoke($this->service, 106.5, -7.5, $feature));
    }
}