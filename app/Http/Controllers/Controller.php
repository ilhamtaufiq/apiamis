<?php

namespace App\Http\Controllers;

use Illuminate\Foundation\Auth\Access\AuthorizesRequests;
use Illuminate\Foundation\Bus\DispatchesJobs;
use Illuminate\Foundation\Validation\ValidatesRequests;
use Illuminate\Routing\Controller as BaseController;

/**
 * @OA\Info(
 *      version="1.0.0",
 *      title="apiamis",
 *      description="API Documentation untuk Sistem Manajemen Kegiatan & Pekerjaan",
 *      @OA\Contact(
 *          email="support@disperkim.id"
 *      ),
 * )
 *
 * @OA\SecurityScheme(
 *      type="http",
 *      securityScheme="bearerAuth",
 *      scheme="bearer",
 *      bearerFormat="Sanctum",
 *      description="Token dari POST /api/auth/login. Format header: Bearer <token>"
 * )
 *
 * @OA\SecurityScheme(
 *      type="apiKey",
 *      in="header",
 *      securityScheme="apiKeyAuth",
 *      name="X-API-KEY",
 *      description="Use your personal API Key in the X-API-KEY header"
 * )
 *
 * @OA\Server(
 *      url=L5_SWAGGER_CONST_HOST,
 *      description="API Server"
 * )
 */
class Controller extends BaseController
{
    use AuthorizesRequests, DispatchesJobs, ValidatesRequests;
}
