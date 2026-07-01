<?php

require __DIR__.'/../vendor/autoload.php';

use PhpOffice\PhpSpreadsheet\Spreadsheet;
use PhpOffice\PhpSpreadsheet\Style\Alignment;
use PhpOffice\PhpSpreadsheet\Style\Border;
use PhpOffice\PhpSpreadsheet\Writer\Xlsx;

$spreadsheet = new Spreadsheet();
$sheet = $spreadsheet->getActiveSheet();
$sheet->setTitle('Ringkasan Kontrak');

$sheet->mergeCells('A1:D1');
$sheet->setCellValue('A1', 'RINGKASAN KONTRAK');
$sheet->getStyle('A1')->getFont()->setBold(true)->setSize(14);
$sheet->getStyle('A1')->getAlignment()->setHorizontal(Alignment::HORIZONTAL_CENTER);

$rows = [
    ['Nama Paket', '{nama_paket}'],
    ['Sub Kegiatan', '{nama_sub_kegiatan}'],
    ['Kecamatan / Desa', '{kecamatan} / {desa}'],
    ['Tahun Anggaran', '{tahun}'],
    ['Sumber Dana', '{sumber_dana}'],
    ['Nilai Kontrak', '{nilai_kontrak}'],
    ['Terbilang', '{nilai_kontrak_terbilang}'],
    ['Nomor SPPBJ', '{nomor_sppbj}'],
    ['Tanggal SPPBJ', '{tgl_sppbj}'],
    ['Nomor SPK', '{nomor_spk}'],
    ['Tanggal SPK', '{tgl_spk}'],
    ['Nomor SPMK', '{nomor_spmk}'],
    ['Tanggal Mulai', '{tanggal_mulai}'],
    ['Tanggal Selesai', '{tanggal_selesai}'],
    ['Masa Pelaksanaan', '{masa}'],
    ['Penyedia', '{nama_penyedia}'],
    ['Direktur', '{nama_direktur}'],
    ['Alamat Penyedia', '{alamat_penyedia}'],
    ['Bank / Rekening', '{bank_penyedia} / {rekening_penyedia}'],
];

$row = 3;
foreach ($rows as $item) {
    $sheet->setCellValue('A'.$row, $item[0]);
    $sheet->setCellValue('B'.$row, $item[1]);
    $sheet->getStyle('A'.$row)->getFont()->setBold(true);
    $row++;
}

$sheet->getColumnDimension('A')->setWidth(24);
$sheet->getColumnDimension('B')->setWidth(60);
$sheet->getStyle('A3:B'.($row - 1))->getBorders()->getAllBorders()->setBorderStyle(Border::BORDER_THIN);

$path = __DIR__.'/../storage/app/templates/ringkasan_kontrak_template.xlsx';
$writer = new Xlsx($spreadsheet);
$writer->save($path);

echo "Created: {$path}\n";