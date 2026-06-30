<?php

namespace App\Http\Controllers;

use App\Models\BroadcastHistory;
use App\Models\Pekerjaan;
use App\Models\User;
use App\Notifications\AppNotification;
use App\Services\UserPekerjaanCompletenessService;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\DB;

class UserPekerjaanController extends Controller
{
    /**
     * @OA\Get(
     *     path="/api/user-pekerjaan",
     *     summary="List all user-pekerjaan assignments",
     *     tags={"Access Control (User-Pekerjaan Assignment)"},
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function index()
    {
        $assignments = DB::table('user_pekerjaan')
            ->join('users', 'user_pekerjaan.user_id', '=', 'users.id')
            ->join('tbl_pekerjaan', 'user_pekerjaan.pekerjaan_id', '=', 'tbl_pekerjaan.id')
            ->select(
                'user_pekerjaan.id',
                'user_pekerjaan.user_id',
                'user_pekerjaan.pekerjaan_id',
                'users.name as user_name',
                'users.email as user_email',
                'tbl_pekerjaan.nama_paket as pekerjaan_nama',
                'tbl_pekerjaan.pagu as pekerjaan_pagu',
                'user_pekerjaan.created_at'
            )
            ->orderBy('user_pekerjaan.created_at', 'desc')
            ->get();

        return response()->json([
            'status' => 'success',
            'data' => $assignments
        ]);
    }

    /**
     * @OA\Post(
     *     path="/api/user-pekerjaan",
     *     summary="Assign user to multiple pekerjaan",
     *     tags={"Access Control (User-Pekerjaan Assignment)"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\RequestBody(
     *         required=true,
     *         @OA\JsonContent(
     *             required={"user_id", "pekerjaan_ids"},
     *             @OA\Property(property="user_id", type="integer"),
     *             @OA\Property(property="pekerjaan_ids", type="array", @OA\Items(type="integer"))
     *         )
     *     ),
     *     @OA\Response(response=201, description="Assigned")
     * )
     */
    public function store(Request $request)
    {
        $request->validate([
            'user_id' => 'required|exists:users,id',
            'pekerjaan_ids' => 'required|array',
            'pekerjaan_ids.*' => 'exists:tbl_pekerjaan,id'
        ]);

        $user = User::findOrFail($request->user_id);
        
        // Sync pekerjaan (this will add new ones and remove unselected)
        $user->assignedPekerjaan()->syncWithoutDetaching($request->pekerjaan_ids);

        // Default to pengawas for new assignees; konsultan_pengawas users keep their role only.
        $user->grantPengawasRoleIfEligible();

        // Notify User
        $pekerjaanNames = Pekerjaan::whereIn('id', $request->pekerjaan_ids)->pluck('nama_paket')->toArray();
        $user->notify(new AppNotification(
            'Penugasan Pekerjaan Baru',
            'Anda telah di-assign ke ' . count($request->pekerjaan_ids) . ' pekerjaan baru: ' . implode(', ', $pekerjaanNames),
            '/pekerjaan',
            'info'
        ));

        return response()->json([
            'status' => 'success',
            'message' => 'Pekerjaan berhasil di-assign ke user'
        ], 201);
    }

    /**
     * @OA\Delete(
     *     path="/api/user-pekerjaan/{id}",
     *     summary="Remove assignment",
     *     tags={"Access Control (User-Pekerjaan Assignment)"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Deleted")
     * )
     */
    public function destroy($id)
    {
        $deleted = DB::table('user_pekerjaan')->where('id', $id)->delete();

        if (!$deleted) {
            return response()->json([
                'status' => 'error',
                'message' => 'Assignment tidak ditemukan'
            ], 404);
        }

        return response()->json([
            'status' => 'success',
            'message' => 'Assignment berhasil dihapus'
        ]);
    }

    /**
     * @OA\Get(
     *     path="/api/user-pekerjaan/user/{userId}",
     *     summary="Get assignments by user",
     *     tags={"Access Control (User-Pekerjaan Assignment)"},
     *     @OA\Parameter(name="userId", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function byUser($userId)
    {
        $user = User::with('assignedPekerjaan.kecamatan', 'assignedPekerjaan.desa', 'assignedPekerjaan.kegiatan')
            ->findOrFail($userId);

        return response()->json([
            'status' => 'success',
            'data' => [
                'user' => [
                    'id' => $user->id,
                    'name' => $user->name,
                    'email' => $user->email
                ],
                'pekerjaan' => $user->assignedPekerjaan
            ]
        ]);
    }

    /**
     * @OA\Get(
     *     path="/api/user-pekerjaan/pekerjaan/{pekerjaanId}",
     *     summary="Get assignments by pekerjaan",
     *     tags={"Access Control (User-Pekerjaan Assignment)"},
     *     @OA\Parameter(name="pekerjaanId", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function byPekerjaan($pekerjaanId)
    {
        $pekerjaan = Pekerjaan::with('assignedUsers')
            ->findOrFail($pekerjaanId);

        return response()->json([
            'status' => 'success',
            'data' => [
                'pekerjaan' => [
                    'id' => $pekerjaan->id,
                    'nama_paket' => $pekerjaan->nama_paket,
                    'pagu' => $pekerjaan->pagu
                ],
                'users' => $pekerjaan->assignedUsers
            ]
        ]);
    }

    /**
     * @OA\Get(
     *     path="/api/user-pekerjaan/available-users",
     *     summary="Get users available for assignment (non-admin)",
     *     tags={"Access Control (User-Pekerjaan Assignment)"},
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function availableUsers(Request $request)
    {
        $search = $request->query('search');

        $users = User::whereDoesntHave('roles', function ($query) {
                $query->where('name', 'admin');
            })
            ->when($search, function ($query, $search) {
                $term = '%' . $search . '%';
                $query->where(function ($q) use ($term) {
                    $q->where('name', 'like', $term)
                        ->orWhere('email', 'like', $term);
                });
            })
            ->select('id', 'name', 'email')
            ->orderBy('name')
            ->get();

        return response()->json([
            'status' => 'success',
            'data' => $users
        ]);
    }

    public function completenessGaps(Request $request, UserPekerjaanCompletenessService $service)
    {
        $request->validate([
            'gaps' => 'nullable|array',
            'gaps.*' => 'in:foto,penerima,progress',
            'tahun' => 'nullable|integer|min:2000|max:2100',
        ]);

        $result = $service->analyze(
            $request->query('gaps'),
            $request->query('tahun') ? (int) $request->query('tahun') : null,
        );

        return response()->json([
            'status' => 'success',
            'data' => $result,
        ]);
    }

    public function broadcastReminders(Request $request, UserPekerjaanCompletenessService $service)
    {
        $request->validate([
            'gaps' => 'nullable|array',
            'gaps.*' => 'in:foto,penerima,progress',
            'tahun' => 'nullable|integer|min:2000|max:2100',
            'user_ids' => 'nullable|array',
            'user_ids.*' => 'exists:users,id',
            'title' => 'nullable|string|max:255',
            'message_prefix' => 'nullable|string|max:1000',
            'notification_type' => 'nullable|in:info,success,warning,error',
            'send_email' => 'nullable|boolean',
        ]);

        $analysis = $service->analyze(
            $request->input('gaps'),
            $request->input('tahun') ? (int) $request->input('tahun') : null,
        );

        $users = $analysis['users'];
        if ($request->filled('user_ids')) {
            $allowedIds = collect($request->input('user_ids'))->map(fn ($id) => (int) $id)->all();
            $users = array_values(array_filter(
                $users,
                fn (array $row) => in_array($row['user_id'], $allowedIds, true),
            ));
        }

        if ($users === []) {
            return response()->json([
                'status' => 'error',
                'message' => 'Tidak ada pengawas dengan data belum lengkap untuk dikirimi pengingat',
            ], 404);
        }

        $title = $request->input('title') ?? 'Pengingat Kelengkapan Data Pekerjaan';
        $notificationType = $request->input('notification_type') ?? 'warning';
        $messagePrefix = $request->input('message_prefix');
        $sendEmail = $request->boolean('send_email');
        $sentCount = 0;
        $emailSentCount = 0;
        $emailFailedCount = 0;
        $emailSkippedCount = 0;
        $smtpUnavailable = false;
        $emailRecipients = [];

        foreach ($users as $userRow) {
            $recipient = User::find($userRow['user_id']);
            if (! $recipient) {
                continue;
            }

            $message = $service->buildReminderMessage($userRow, $messagePrefix);
            $firstPekerjaanId = isset($userRow['pekerjaan'][0]['pekerjaan_id'])
                ? (int) $userRow['pekerjaan'][0]['pekerjaan_id']
                : null;
            $url = $firstPekerjaanId ? "/pekerjaan/{$firstPekerjaanId}" : '/pekerjaan';
            $actionUrl = $service->pengawasActionUrl($firstPekerjaanId);

            $history = BroadcastHistory::create([
                'title' => $title,
                'message' => $message,
                'type' => 'single',
                'notification_type' => $notificationType,
                'url' => $url,
                'is_banner' => false,
                'recipient_count' => 1,
            ]);

            $recipient->notify(new AppNotification(
                $title,
                $message,
                $url,
                $notificationType,
                false,
                $history->id,
            ));

            $sentCount++;

            if ($sendEmail) {
                $emailResult = $service->sendReminderEmail($recipient, $userRow, $title, $message, $actionUrl);

                if ($emailResult['sent']) {
                    $emailSentCount++;
                    if (! empty($emailResult['email'])) {
                        $emailRecipients[] = $emailResult['email'];
                    }
                } elseif ($emailResult['skipped_reason'] === 'smtp_disabled') {
                    $smtpUnavailable = true;
                    $emailSkippedCount++;
                } elseif ($emailResult['skipped_reason'] === 'send_failed') {
                    $emailFailedCount++;
                } else {
                    $emailSkippedCount++;
                }
            }
        }

        $responseMessage = 'Pengingat kelengkapan berhasil dikirim';
        if ($sendEmail) {
            if ($smtpUnavailable && $emailSentCount === 0) {
                $responseMessage .= '. Email tidak terkirim karena SMTP belum diaktifkan';
            } elseif ($emailFailedCount > 0) {
                $responseMessage .= ". Email terkirim ke {$emailSentCount} pengawas, {$emailFailedCount} gagal";
            } elseif ($emailSentCount > 0) {
                $responseMessage .= ". Email terkirim ke {$emailSentCount} pengawas";
            }
        }

        return response()->json([
            'status' => 'success',
            'message' => $responseMessage,
            'recipient_count' => $sentCount,
            'email_sent_count' => $emailSentCount,
            'email_failed_count' => $emailFailedCount,
            'email_skipped_count' => $emailSkippedCount,
            'send_email' => $sendEmail,
            'smtp_unavailable' => $smtpUnavailable,
            'email_recipients' => array_values(array_unique($emailRecipients)),
            'action_url_sample' => $users !== [] ? $service->pengawasActionUrl(
                isset($users[0]['pekerjaan'][0]['pekerjaan_id']) ? (int) $users[0]['pekerjaan'][0]['pekerjaan_id'] : null
            ) : null,
        ]);
    }
}
