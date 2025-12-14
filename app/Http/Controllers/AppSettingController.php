<?php

namespace App\Http\Controllers;

use App\Models\AppSetting;
use App\Http\Resources\AppSettingResource;
use Illuminate\Http\Request;
use Illuminate\Support\Str;

class AppSettingController extends Controller
{
    /**
     * Get all app settings
     */
    public function index()
    {
        $settings = AppSetting::all();
        return AppSettingResource::collection($settings);
    }

    /**
     * Update app settings (handles both text and file uploads)
     */
    public function store(Request $request)
    {
        $request->validate([
            'app_name' => 'nullable|string|max:255',
            'app_description' => 'nullable|string|max:500',
            'tahun_anggaran' => 'nullable|string|max:4',
            'logo' => 'nullable|file|mimes:jpg,jpeg,png,svg|max:2048',
            'favicon' => 'nullable|file|mimes:jpg,jpeg,png,svg,ico|max:1024',
        ]);

        $updatedSettings = [];

        // Handle text settings
        if ($request->has('app_name')) {
            $setting = AppSetting::setValue('app_name', $request->app_name, 'text');
            $updatedSettings[] = $setting;
        }

        if ($request->has('app_description')) {
            $setting = AppSetting::setValue('app_description', $request->app_description, 'text');
            $updatedSettings[] = $setting;
        }

        if ($request->has('tahun_anggaran')) {
            $setting = AppSetting::setValue('tahun_anggaran', $request->tahun_anggaran, 'text');
            $updatedSettings[] = $setting;
        }

        // Handle file uploads
        if ($request->hasFile('logo')) {
            $setting = AppSetting::updateOrCreate(
                ['key' => 'logo'],
                ['type' => 'file', 'value' => null]
            );
            $setting->clearMediaCollection('app-settings');
            $setting->addMediaFromRequest('logo')
                ->usingFileName('logo_' . Str::uuid() . '.' . $request->file('logo')->getClientOriginalExtension())
                ->toMediaCollection('app-settings');
            $updatedSettings[] = $setting->fresh();
        }

        if ($request->hasFile('favicon')) {
            $setting = AppSetting::updateOrCreate(
                ['key' => 'favicon'],
                ['type' => 'file', 'value' => null]
            );
            $setting->clearMediaCollection('app-settings');
            $setting->addMediaFromRequest('favicon')
                ->usingFileName('favicon_' . Str::uuid() . '.' . $request->file('favicon')->getClientOriginalExtension())
                ->toMediaCollection('app-settings');
            $updatedSettings[] = $setting->fresh();
        }

        // Return all settings
        $allSettings = AppSetting::all();
        return AppSettingResource::collection($allSettings);
    }
}
