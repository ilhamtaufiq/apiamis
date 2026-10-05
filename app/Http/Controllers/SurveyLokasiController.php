<?php

namespace App\Http\Controllers;

use App\Http\Resources\SurveyLokasiResource;
use App\Models\SurveyLokasi;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\Validator;

class SurveyLokasiController extends Controller
{
    /**
     * Display a listing of the resource.
     */
    public function index(Request $request)
    {
        $query = SurveyLokasi::with(['kecamatan', 'desa', 'user', 'verifier', 'media', 'tugas']);

        if ($request->filled('jenis')) {
            $query->where('jenis', $request->jenis);
        }

        if ($request->filled('status')) {
            $query->where('status', $request->status);
        }

        if ($request->filled('kecamatan_id')) {
            $query->where('kecamatan_id', $request->kecamatan_id);
        }

        if ($request->filled('desa_id')) {
            $query->where('desa_id', $request->desa_id);
        }

        if ($request->filled('tugas_id')) {
            $query->where('tugas_id', $request->tugas_id);
        }

        if ($request->filled('search')) {
            $search = '%' . $request->search . '%';
            $query->where(function ($q) use ($search) {
                $q->where('nama_lokasi', 'LIKE', $search)
                    ->orWhere('alamat', 'LIKE', $search);
            });
        }

        $perPage = (int) $request->get('per_page', 15);
        $perPage = max(1, min($perPage, 100));

        $surveys = $query->latest()->paginate($perPage);

        return SurveyLokasiResource::collection($surveys);
    }

    /**
     * Statistik jumlah survei per jenis dan per status.
     */
    public function stats()
    {
        $byJenis = [
            'spam_perpipaan' => SurveyLokasi::where('jenis', 'spam_perpipaan')->count(),
            'spam_pengeboran' => SurveyLokasi::where('jenis', 'spam_pengeboran')->count(),
            'mck_individu' => SurveyLokasi::where('jenis', 'mck_individu')->count(),
            'mck_komunal' => SurveyLokasi::where('jenis', 'mck_komunal')->count(),
        ];

        $byStatus = [
            'diajukan' => SurveyLokasi::where('status', 'diajukan')->count(),
            'diverifikasi' => SurveyLokasi::where('status', 'diverifikasi')->count(),
            'ditolak' => SurveyLokasi::where('status', 'ditolak')->count(),
        ];

        return response()->json([
            'by_jenis' => $byJenis,
            'by_status' => $byStatus,
            'total' => array_sum($byStatus),
        ]);
    }

    /**
     * Peran yang boleh mengisi survey lapangan (selain admin).
     */
    public const SURVEY_ROLES = ['tfl', 'operator', 'pengawas', 'konsultan_pengawas'];

    private function canSurvey($user): bool
    {
        if (!$user) {
            return false;
        }
        if ($user->hasRole('admin')) {
            return true;
        }

        return $user->hasRole(self::SURVEY_ROLES);
    }

    private function denySurveyRole()
    {
        return response()->json([
            'message' => 'Forbidden. Hanya admin, TFL, operator, pengawas, atau konsultan pengawas yang dapat mengisi survey.',
        ], 403);
    }

    /**
     * Normalisasi `detail` yang dikirim sebagai string JSON (mode multipart
     * + foto dari aplikasi survey) menjadi array sebelum validasi.
     */
    private function normalizeDetailInput(Request $request): void
    {
        $raw = $request->get('detail');
        if (is_string($raw) && $raw !== '') {
            $decoded = json_decode($raw, true);
            if (is_array($decoded)) {
                $request->merge(['detail' => $decoded]);
            }
        }
    }

    /**
     * Aturan validasi store/update.
     */
    private function rules(bool $isUpdate = false): array
    {
        $required = $isUpdate ? 'sometimes|required' : 'required';

        return [
            'jenis' => $required . '|in:spam_perpipaan,spam_pengeboran,mck_individu,mck_komunal',
            'tugas_id' => 'nullable|exists:tbl_survey_tugas,id',
            'nama_lokasi' => $required . '|string|max:255',
            'kecamatan_id' => 'nullable|exists:tbl_kecamatan,id',
            'desa_id' => 'nullable|exists:tbl_desa,id',
            'alamat' => 'nullable|string',
            'latitude' => 'nullable|numeric|between:-90,90',
            'longitude' => 'nullable|numeric|between:-180,180',
            'detail' => 'nullable|array',
            'detail.sumber_air' => 'nullable|string|max:255',
            'detail.debit_liter_detik' => 'nullable|numeric|min:0',
            'detail.jumlah_kk' => 'nullable|integer|min:0',
            'detail.kebutuhan' => 'nullable|in:baru,rehab,perluasan',
            'detail.tipe' => 'nullable|in:individu,komunal',
            'detail.jumlah_bilik' => 'nullable|integer|min:0',
            'detail.kedalaman_rencana_m' => 'nullable|numeric|min:0',
            // ── Formulir SPAM Perdesaan ──
            'detail.tanggal_survei' => 'nullable|date',
            'detail.nama_surveyor_tim' => 'nullable|string|max:255',
            'detail.dusun' => 'nullable|string|max:255',
            'detail.rt' => 'nullable|string|max:20',
            'detail.rw' => 'nullable|string|max:20',
            'detail.broncaptering_lat' => 'nullable|numeric|between:-90,90',
            'detail.broncaptering_lng' => 'nullable|numeric|between:-180,180',
            'detail.broncaptering_elevasi' => 'nullable|numeric',
            'detail.reservoir_lat' => 'nullable|numeric|between:-90,90',
            'detail.reservoir_lng' => 'nullable|numeric|between:-180,180',
            'detail.reservoir_elevasi' => 'nullable|numeric',
            'detail.sumber_air_jenis' => 'nullable|in:mata_air_terjun,rembesan,umbul,sungai,sumur_bor',
            'detail.debit_hujan_lps' => 'nullable|numeric|min:0',
            'detail.debit_hujan_bulan' => 'nullable|string|max:50',
            'detail.debit_kemarau_lps' => 'nullable|numeric|min:0',
            'detail.debit_kemarau_bulan' => 'nullable|string|max:50',
            'detail.kejernihan' => 'nullable|in:jernih,keruh,berwarna',
            'detail.bau_rasa' => 'nullable|in:berbau_berasa,tidak',
            'detail.ph' => 'nullable|numeric|min:0|max:14',
            'detail.lahan_broncaptering_status' => 'nullable|in:tanah_desa,milik_warga,hutan',
            'detail.elevasi_kelayakan' => 'nullable|in:gravitasi,pompa',
            'detail.jarak_sumber_reservoir_m' => 'nullable|numeric|min:0',
            'detail.reservoir_lahan_status' => 'nullable|in:tanah_desa,hibah_warga,lainnya',
            'detail.reservoir_lahan_lainnya' => 'nullable|string|max:255',
            'detail.topografi' => 'nullable|in:datar,miring,rawan_longsor',
            'detail.selisih_elevasi_m' => 'nullable|numeric',
            'detail.akses_material' => 'nullable|in:mobil_truk,motor_roda3,jalan_kaki',
            'detail.pipa_trunk_m' => 'nullable|numeric|min:0',
            'detail.pipa_cabang_m' => 'nullable|numeric|min:0',
            'detail.lintas_tanah_m' => 'nullable|numeric|min:0',
            'detail.lintas_paving_m' => 'nullable|numeric|min:0',
            'detail.lintas_aspal_m' => 'nullable|numeric|min:0',
            'detail.lintas_sungai_titik' => 'nullable|integer|min:0',
            'detail.lintas_sungai_lebar_m' => 'nullable|numeric|min:0',
            'detail.washout_titik' => 'nullable|integer|min:0',
            'detail.air_valve_titik' => 'nullable|integer|min:0',
            'detail.total_jiwa' => 'nullable|integer|min:0',
            'detail.sumber_eksisting' => 'nullable|in:sumur,irigasi,beli',
            'detail.kesediaan_pelanggan' => 'nullable|in:ya,tidak',
            'detail.kesediaan_persen' => 'nullable|numeric|min:0|max:100',
            'detail.kesediaan_iuran' => 'nullable|in:setuju,tidak_setuju',
            'detail.tarif_perkiraan' => 'nullable|numeric|min:0',
            'detail.bnba' => 'nullable|string|max:10000',
            // ── Formulir SPAM Sumur Bor ──
            'detail.sumur_lat' => 'nullable|numeric|between:-90,90',
            'detail.sumur_lng' => 'nullable|numeric|between:-180,180',
            'detail.sumur_elevasi' => 'nullable|numeric',
            'detail.ku1_lat' => 'nullable|numeric|between:-90,90',
            'detail.ku1_lng' => 'nullable|numeric|between:-180,180',
            'detail.ku2_lat' => 'nullable|numeric|between:-90,90',
            'detail.ku2_lng' => 'nullable|numeric|between:-180,180',
            'detail.lahan_sumur_status' => 'nullable|in:tanah_kas_desa,hibah_warga,lainnya',
            'detail.lahan_sumur_lainnya' => 'nullable|string|max:255',
            'detail.lahan_panjang_m' => 'nullable|numeric|min:0',
            'detail.lahan_lebar_m' => 'nullable|numeric|min:0',
            'detail.akuifer_kedalaman_m' => 'nullable|numeric|min:0',
            'detail.sumur_warga_kedalaman_m' => 'nullable|numeric|min:0',
            'detail.air_warga_kualitas' => 'nullable|in:jernih,berbau,asin_payau,besi_mangan',
            'detail.listrik_jarak_m' => 'nullable|numeric|min:0',
            'detail.listrik_daya' => 'nullable|in:belum_ada,900,1300,2200',
            'detail.akses_rig' => 'nullable|in:truk,pickup,portable',
            'detail.tanah_menara' => 'nullable|in:keras,sawah_rawa,miring_tebing',
            'detail.menara_tinggi' => 'nullable|in:3,5,6',
            'detail.toren_kapasitas' => 'nullable|in:1000,2000,4000,lainnya',
            'detail.toren_kapasitas_lainnya' => 'nullable|string|max:50',
            'detail.toren_bahan' => 'nullable|in:pe,stainless',
            'detail.jumlah_ku_titik' => 'nullable|integer|min:0',
            'detail.ku_rincian' => 'nullable|string|max:5000',
            'detail.kran_per_titik' => 'nullable|in:2,4',
            'detail.drainase' => 'nullable|in:ada_saluran,perlu_resapan',
            'detail.kesediaan_kelompok' => 'nullable|in:ya,tidak',
            'detail.iuran_listrik' => 'nullable|in:ya,tidak',
            // ── Formulir MCK ──
            'detail.mck_lat' => 'nullable|numeric|between:-90,90',
            'detail.mck_lng' => 'nullable|numeric|between:-180,180',
            'detail.mck_elevasi' => 'nullable|numeric',
            'detail.jumlah_pintu' => 'nullable|in:1,2,3,4,lainnya',
            'detail.jumlah_pintu_lainnya' => 'nullable|string|max:100',
            'detail.bilik1_fungsi' => 'nullable|in:jongkok,duduk,mandi',
            'detail.bilik2_fungsi' => 'nullable|in:jongkok,duduk,mandi',
            'detail.bilik3_fungsi' => 'nullable|in:jongkok,duduk,mandi',
            'detail.bilik4_fungsi' => 'nullable|in:jongkok,duduk,mandi',
            'detail.kloset_jenis' => 'nullable|in:leher_angsa,duduk',
            'detail.wudhu_ada' => 'nullable|in:ada,tidak',
            'detail.wudhu_keran' => 'nullable|integer|min:0',
            'detail.wudhu_desain' => 'nullable|in:dinding_luar,kanopi,duduk_beton',
            'detail.mck_sumber_air' => 'nullable|in:spam_desa,sumur,mata_air',
            'detail.toren_menara' => 'nullable|in:ada,tidak',
            'detail.toren_dak' => 'nullable|in:tidak,500,1000,2000',
            'detail.septik_jenis' => 'nullable|in:biofilter,konvensional',
            'detail.septik_bio_kapasitas_m3' => 'nullable|numeric|min:0',
            'detail.septik_bio_pengguna' => 'nullable|integer|min:0',
            'detail.septik_panjang_m' => 'nullable|numeric|min:0',
            'detail.septik_lebar_m' => 'nullable|numeric|min:0',
            'detail.septik_dalam_m' => 'nullable|numeric|min:0',
            'detail.resapan_jenis' => 'nullable|in:sumur,trench,drainase',
            'detail.resapan_diameter_m' => 'nullable|numeric|min:0',
            'detail.resapan_dalam_m' => 'nullable|numeric|min:0',
            'detail.tanah_jenis' => 'nullable|in:pasir,liat,batuan',
            'detail.muka_air_tanah_m' => 'nullable|numeric|min:0',
            'detail.jarak_septik_sumur_m' => 'nullable|numeric|min:0',
            'detail.nama_kk' => 'nullable|string|max:255',
            'detail.anggota_jiwa' => 'nullable|integer|min:0',
            'detail.status_ekonomi' => 'nullable|in:mbr,non_mbr',
            'detail.target_warga_kk' => 'nullable|integer|min:0',
            'detail.target_warga_jiwa' => 'nullable|integer|min:0',
            'detail.target_jamaah' => 'nullable|integer|min:0',
            'detail.target_santri' => 'nullable|integer|min:0',
            'detail.pengelola' => 'nullable|in:musala,ksm,bumdes',
            'detail.dok_foto_broncaptering' => 'nullable|boolean',
            'detail.dok_foto_reservoir' => 'nullable|boolean',
            'detail.dok_peta_jalur' => 'nullable|boolean',
            'detail.dok_surat_hibah' => 'nullable|boolean',
            'detail.dok_bnba' => 'nullable|boolean',
            'foto' => 'nullable|array',
            'foto.*' => 'file|mimes:jpg,jpeg,png,webp,gif,pdf,doc,docx,xls,xlsx|max:10240',
            'foto_kategori' => 'nullable|array',
            'foto_kategori.*' => 'nullable|string|max:50',
        ];
    }

    /**
     * Simpan foto multipart ke koleksi 'foto'.
     * Kategori lampiran (sejajar dengan urutan file) dibaca dari
     * `foto_kategori[]` dan disimpan sebagai custom property media.
     */
    private function storeFotos(Request $request, SurveyLokasi $survey): void
    {
        if (!$request->hasFile('foto')) {
            return;
        }

        $files = $request->file('foto');
        $files = is_array($files) ? $files : [$files];
        $kategoris = $request->input('foto_kategori', []);
        if (!is_array($kategoris)) {
            $kategoris = [$kategoris];
        }

        foreach (array_values($files) as $i => $file) {
            if ($file && $file->isValid()) {
                $adder = $survey->addMedia($file);
                $kategori = trim((string) ($kategoris[$i] ?? ''));
                if ($kategori !== '') {
                    $adder->withCustomProperties(['kategori' => $kategori]);
                }
                $adder->toMediaCollection('foto');
            }
        }
    }

    /**
     * Store a newly created resource in storage.
     */
    public function store(Request $request)
    {
        $user = auth()->user();
        if (!$user) {
            return response()->json(['message' => 'Unauthorized'], 401);
        }

        if (!$this->canSurvey($user)) {
            return $this->denySurveyRole();
        }

        if ($request->filled('tugas_id')) {
            $earlyTugas = \App\Models\SurveyTugas::find($request->tugas_id);
            if ($earlyTugas && $earlyTugas->jenis && empty($request->jenis)) {
                $request->merge(['jenis' => $earlyTugas->jenis]);
            }
        }

        $this->normalizeDetailInput($request);
        $validator = Validator::make($request->all(), $this->rules());

        if ($validator->fails()) {
            return response()->json(['message' => 'Validation error', 'errors' => $validator->errors()], 422);
        }

        $tugas = null;
        $jenis = $request->jenis;
        if ($request->filled('tugas_id')) {
            $tugas = \App\Models\SurveyTugas::find($request->tugas_id);
            if (!$tugas) {
                return response()->json(['message' => 'Tugas survey tidak ditemukan.'], 404);
            }
            $isAdmin = $user->hasRole('admin');
            if (!$isAdmin && $tugas->assignee_id !== $user->id) {
                return response()->json(['message' => 'Forbidden'], 403);
            }
            if ($tugas->jenis) {
                if (empty($jenis)) {
                    $jenis = $tugas->jenis;
                } elseif ($jenis !== $tugas->jenis) {
                    return response()->json([
                        'message' => 'Validation error',
                        'errors' => ['jenis' => ['Jenis survey harus sesuai tugas.']],
                    ], 422);
                }
            }
        }

        $survey = SurveyLokasi::create([
            'user_id' => $user->id,
            'tugas_id' => $request->tugas_id,
            'jenis' => $jenis,
            'nama_lokasi' => $request->nama_lokasi,
            'kecamatan_id' => $request->kecamatan_id,
            'desa_id' => $request->desa_id,
            'alamat' => $request->alamat,
            'latitude' => $request->latitude,
            'longitude' => $request->longitude,
            'detail' => $request->detail,
            'status' => 'diajukan',
        ]);

        $this->storeFotos($request, $survey);

        if ($tugas && $tugas->status === 'ditugaskan') {
            $tugas->update(['status' => 'dikerjakan']);
        }

        return (new SurveyLokasiResource($survey->load(['kecamatan', 'desa', 'user', 'verifier', 'media', 'tugas'])))
            ->response()
            ->setStatusCode(201);
    }

    /**
     * Display the specified resource.
     */
    public function show(SurveyLokasi $surveyLokasi)
    {
        $surveyLokasi->load(['kecamatan', 'desa', 'user', 'verifier', 'media', 'tugas']);

        return new SurveyLokasiResource($surveyLokasi);
    }

    /**
     * Update the specified resource in storage.
     */
    public function update(Request $request, SurveyLokasi $surveyLokasi)
    {
        $user = auth()->user();
        if (!$user) {
            return response()->json(['message' => 'Unauthorized'], 401);
        }

        $isAdmin = $user->hasRole('admin');

        if (!$isAdmin && !$this->canSurvey($user)) {
            return $this->denySurveyRole();
        }

        if (!$isAdmin && $surveyLokasi->user_id !== $user->id) {
            return response()->json(['message' => 'Forbidden'], 403);
        }

        if (!$isAdmin && $surveyLokasi->status !== 'diajukan') {
            return response()->json(['message' => 'Hanya survei berstatus diajukan yang dapat diubah.'], 403);
        }

        $this->normalizeDetailInput($request);
        $validator = Validator::make($request->all(), $this->rules(true));

        if ($validator->fails()) {
            return response()->json(['message' => 'Validation error', 'errors' => $validator->errors()], 422);
        }

        $tugasId = $request->has('tugas_id') ? $request->tugas_id : $surveyLokasi->tugas_id;
        if ($tugasId) {
            $tugas = \App\Models\SurveyTugas::find($tugasId);
            if (!$tugas) {
                return response()->json(['message' => 'Tugas survey tidak ditemukan.'], 404);
            }
            if (!$isAdmin && $tugas->assignee_id !== $user->id) {
                return response()->json(['message' => 'Forbidden'], 403);
            }
            if ($tugas->jenis) {
                $jenisBaru = $request->has('jenis') ? $request->jenis : $surveyLokasi->jenis;
                if (empty($jenisBaru)) {
                    $request->merge(['jenis' => $tugas->jenis]);
                } elseif ($jenisBaru !== $tugas->jenis) {
                    return response()->json([
                        'message' => 'Validation error',
                        'errors' => ['jenis' => ['Jenis survey harus sesuai tugas.']],
                    ], 422);
                }
            }
        }

        $surveyLokasi->update($request->only([
            'tugas_id',
            'jenis',
            'nama_lokasi',
            'kecamatan_id',
            'desa_id',
            'alamat',
            'latitude',
            'longitude',
            'detail',
        ]));

        $this->storeFotos($request, $surveyLokasi);

        return new SurveyLokasiResource($surveyLokasi->load(['kecamatan', 'desa', 'user', 'verifier', 'media', 'tugas']));
    }

    /**
     * Remove the specified resource from storage.
     */
    public function destroy(SurveyLokasi $surveyLokasi)
    {
        $user = auth()->user();
        if (!$user) {
            return response()->json(['message' => 'Unauthorized'], 401);
        }

        $isAdmin = $user->hasRole('admin');

        if (!$isAdmin && !$this->canSurvey($user)) {
            return $this->denySurveyRole();
        }

        if (!$isAdmin && $surveyLokasi->user_id !== $user->id) {
            return response()->json(['message' => 'Forbidden'], 403);
        }

        if (!$isAdmin && $surveyLokasi->status !== 'diajukan') {
            return response()->json(['message' => 'Hanya survei berstatus diajukan yang dapat dihapus.'], 403);
        }

        $surveyLokasi->clearMediaCollection('foto');
        $surveyLokasi->delete();

        return response()->json(['message' => 'Survei lokasi berhasil dihapus.']);
    }

    /**
     * Verifikasi survei (admin only via route middleware).
     */
    public function verifikasi(Request $request, SurveyLokasi $surveyLokasi)
    {
        $validator = Validator::make($request->all(), [
            'status' => 'required|in:diverifikasi,ditolak',
            'catatan_verifikasi' => 'nullable|string|required_if:status,ditolak',
        ], [
            'catatan_verifikasi.required_if' => 'Catatan verifikasi wajib diisi jika survei ditolak.',
        ]);

        if ($validator->fails()) {
            return response()->json(['message' => 'Validation error', 'errors' => $validator->errors()], 422);
        }

        $surveyLokasi->update([
            'status' => $request->status,
            'catatan_verifikasi' => $request->catatan_verifikasi,
            'verified_by' => auth()->id(),
            'verified_at' => now(),
        ]);

        if ($surveyLokasi->tugas_id && $request->status === 'diverifikasi') {
            $tugas = \App\Models\SurveyTugas::find($surveyLokasi->tugas_id);
            if ($tugas) {
                $tugas->update(['status' => 'selesai']);
            }
        }

        return new SurveyLokasiResource($surveyLokasi->load(['kecamatan', 'desa', 'user', 'verifier', 'media', 'tugas']));
    }

    /**
     * Upload foto tambahan.
     */
    public function uploadFoto(Request $request, SurveyLokasi $surveyLokasi)
    {
        $user = auth()->user();
        if (!$user) {
            return response()->json(['message' => 'Unauthorized'], 401);
        }

        $isAdmin = $user->hasRole('admin');

        if (!$isAdmin && !$this->canSurvey($user)) {
            return $this->denySurveyRole();
        }

        if (!$isAdmin && $surveyLokasi->user_id !== $user->id) {
            return response()->json(['message' => 'Forbidden'], 403);
        }

        if (!$isAdmin && $surveyLokasi->status !== 'diajukan') {
            return response()->json(['message' => 'Hanya survei berstatus diajukan yang dapat ditambah fotonya.'], 403);
        }

        $validator = Validator::make($request->all(), [
            'foto' => 'required|file|mimes:jpg,jpeg,png,webp,gif,pdf,doc,docx,xls,xlsx|max:10240',
            'kategori' => 'nullable|string|max:50',
        ]);

        if ($validator->fails()) {
            return response()->json(['message' => 'Validation error', 'errors' => $validator->errors()], 422);
        }

        $adder = $surveyLokasi->addMediaFromRequest('foto');
        $kategori = trim((string) $request->input('kategori', ''));
        if ($kategori !== '') {
            $adder->withCustomProperties(['kategori' => $kategori]);
        }
        $adder->toMediaCollection('foto');

        return new SurveyLokasiResource($surveyLokasi->load(['kecamatan', 'desa', 'user', 'verifier', 'media']));
    }

    /**
     * Hapus satu foto.
     */
    public function deleteFoto(SurveyLokasi $surveyLokasi, $mediaId)
    {
        $user = auth()->user();
        if (!$user) {
            return response()->json(['message' => 'Unauthorized'], 401);
        }

        $isAdmin = $user->hasRole('admin');

        if (!$isAdmin && !$this->canSurvey($user)) {
            return $this->denySurveyRole();
        }

        if (!$isAdmin && $surveyLokasi->user_id !== $user->id) {
            return response()->json(['message' => 'Forbidden'], 403);
        }

        if (!$isAdmin && $surveyLokasi->status !== 'diajukan') {
            return response()->json(['message' => 'Hanya survei berstatus diajukan yang dapat dihapus fotonya.'], 403);
        }

        $media = $surveyLokasi->getMedia('foto')->firstWhere('id', (int) $mediaId);

        if (!$media) {
            return response()->json(['message' => 'Foto tidak ditemukan.'], 404);
        }

        $media->delete();

        return response()->json(['message' => 'Foto berhasil dihapus.']);
    }
}
