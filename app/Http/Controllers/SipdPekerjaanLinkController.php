<?php

namespace App\Http\Controllers;

use App\Models\SipdPekerjaanLink;
use Illuminate\Http\Request;

class SipdPekerjaanLinkController extends Controller
{
    /**
     * Daftar link untuk satu sub kegiatan SIPD.
     * GET /api/sipd-pekerjaan-links?id_sub_bl=X
     */
    public function index(Request $request)
    {
        $idSubBl = (int) $request->query('id_sub_bl');
        abort_unless($idSubBl > 0, 422, 'id_sub_bl wajib diisi');

        $links = SipdPekerjaanLink::where('id_sub_bl', $idSubBl)
            ->get(['id_rinci_sub_bl', 'pekerjaan_id']);

        return response()->json(['data' => $links]);
    }

    /**
     * Set/simpan link.
     * PUT /api/sipd-pekerjaan-links
     * body: { id_sub_bl, id_rinci_sub_bl, pekerjaan_id }
     */
    public function upsert(Request $request)
    {
        $data = $request->validate([
            'id_sub_bl' => 'required|integer|min:1',
            'id_rinci_sub_bl' => 'required|integer|min:1',
            'pekerjaan_id' => 'required|integer|min:1',
        ]);

        $link = SipdPekerjaanLink::updateOrCreate(
            ['id_sub_bl' => $data['id_sub_bl'], 'id_rinci_sub_bl' => $data['id_rinci_sub_bl']],
            ['pekerjaan_id' => $data['pekerjaan_id']]
        );

        return response()->json([
            'data' => [
                'id_rinci_sub_bl' => $link->id_rinci_sub_bl,
                'pekerjaan_id' => $link->pekerjaan_id,
            ],
        ]);
    }

    /**
     * Hapus link (lepas tautan).
     * DELETE /api/sipd-pekerjaan-links
     * body: { id_sub_bl, id_rinci_sub_bl }
     */
    public function destroy(Request $request)
    {
        $data = $request->validate([
            'id_sub_bl' => 'required|integer|min:1',
            'id_rinci_sub_bl' => 'required|integer|min:1',
        ]);

        SipdPekerjaanLink::where('id_sub_bl', $data['id_sub_bl'])
            ->where('id_rinci_sub_bl', $data['id_rinci_sub_bl'])
            ->delete();

        return response()->json(['data' => null]);
    }
}
