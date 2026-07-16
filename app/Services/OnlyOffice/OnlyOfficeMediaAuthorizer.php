<?php

namespace App\Services\OnlyOffice;

use App\Models\Berkas;
use App\Models\Kontrak;
use App\Models\KontrakAddendum;
use App\Models\Pekerjaan;
use App\Models\PuspenMediaShare;
use App\Models\User;
use App\Models\UserDriveItem;
use Spatie\MediaLibrary\MediaCollections\Models\Media;

class OnlyOfficeMediaAuthorizer
{
    public function canAccess(?User $user, Media $media): bool
    {
        if (! $user) {
            return false;
        }

        if ($user->hasRole('admin')) {
            return true;
        }

        $owner = $media->model;
        if (! $owner) {
            return false;
        }

        if ($owner instanceof Berkas) {
            return Pekerjaan::query()
                ->byUserRole()
                ->whereKey($owner->pekerjaan_id)
                ->exists();
        }

        if ($owner instanceof Kontrak) {
            $owner->loadMissing('pekerjaans');

            $pekerjaanIds = $owner->pekerjaans->pluck('id');
            if ($owner->id_pekerjaan) {
                $pekerjaanIds->push($owner->id_pekerjaan);
            }

            return Pekerjaan::query()
                ->byUserRole()
                ->whereIn('id', $pekerjaanIds->unique()->filter()->values())
                ->exists();
        }

        if ($owner instanceof KontrakAddendum) {
            $owner->loadMissing('kontrak.pekerjaans');

            $kontrak = $owner->kontrak;
            if (! $kontrak) {
                return false;
            }

            $pekerjaanIds = $kontrak->pekerjaans->pluck('id');
            if ($kontrak->id_pekerjaan) {
                $pekerjaanIds->push($kontrak->id_pekerjaan);
            }

            return Pekerjaan::query()
                ->byUserRole()
                ->whereIn('id', $pekerjaanIds->unique()->filter()->values())
                ->exists();
        }

        if ($owner instanceof PuspenMediaShare) {
            return $owner->user_id === $user->id;
        }

        if ($owner instanceof UserDriveItem) {
            return $owner->canManage($user);
        }

        return false;
    }

    /**
     * Who may open the document in edit mode.
     * View access is broader; edit is limited to admins, operators, and resource owners.
     */
    public function canEdit(?User $user, Media $media): bool
    {
        if (! $user || ! $this->canAccess($user, $media)) {
            return false;
        }

        if ($user->hasRole('admin') || $user->hasRole('operator')) {
            return true;
        }

        $owner = $media->model;
        if (! $owner) {
            return false;
        }

        if ($owner instanceof UserDriveItem) {
            return $owner->canManage($user);
        }

        if ($owner instanceof PuspenMediaShare) {
            return $owner->user_id === $user->id;
        }

        // Berkas / kontrak: pengawas may edit documents of assigned pekerjaan.
        if ($user->hasRole('pengawas') || $user->hasRole('konsultan_pengawas')) {
            return $this->canAccess($user, $media);
        }

        return false;
    }
}
