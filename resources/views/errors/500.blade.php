@extends('errors.minimal')

@section('title', __('Kesalahan Server'))
@section('code', '500')
@section('message', __('Terjadi gangguan internal pada server API. Tim kami sedang menangani masalah ini.'))
