<?php

$filePath = 'c:\\laragon\\www\\bun\\src\\assets\\geojson\\kecamatan\\id3203_cianjur_simplified.geojson';

if (!file_exists($filePath)) {
    echo "File not found: $filePath\n";
    exit(1);
}

$json = json_decode(file_get_contents($filePath), true);
if (!$json) {
    echo "Failed to decode JSON\n";
    exit(1);
}

$count = 0;
foreach ($json['features'] as &$feature) {
    if (isset($feature['properties']['district'])) {
        $oldD = $feature['properties']['district'];
        $newD = str_replace(' ', '', $oldD);
        if ($oldD !== $newD) {
            $feature['properties']['district'] = $newD;
            $count++;
        }
    }
    if (isset($feature['properties']['village'])) {
        $oldV = $feature['properties']['village'];
        $newV = str_replace(' ', '', $oldV);
        if ($oldV !== $newV) {
            $feature['properties']['village'] = $newV;
            $count++;
        }
    }
}

file_put_contents($filePath, json_encode($json));
echo "Successfully updated GeoJSON. Normalized $count names.\n";
