@extends('errors.minimal')

@section('title', __('Tidak Terautentikasi'))
@section('code', '401')
@section('message', __('Sesi Anda telah kedaluwarsa atau token tidak valid. Silakan masuk kembali untuk melanjutkan.'))
