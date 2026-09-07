{{-- Kop laporan PDF (gaya export pekerjaan): logo kiri + teks instansi + garis. --}}
<table class="kop">
    <tr>
        <td class="kop-logo">
            @if(!empty($logoDataUrl))
                <img src="{{ $logoDataUrl }}" width="52" height="52">
            @endif
        </td>
        <td class="kop-text">
            <div class="kop-line1">PEMERINTAH KABUPATEN CIANJUR</div>
            <div class="kop-line2">DINAS PERUMAHAN DAN KAWASAN PERMUKIMAN</div>
            <div class="kop-line3">Bidang Air Minum dan Sanitasi</div>
        </td>
        <td class="kop-logo"></td>
    </tr>
</table>
<div class="kop-rule"></div>
