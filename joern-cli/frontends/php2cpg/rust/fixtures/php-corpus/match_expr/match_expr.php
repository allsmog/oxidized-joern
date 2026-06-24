<?php

function classify(array $names): string
{
    $kind = match (count($names)) {
        0 => "empty",
        1, 2 => "few",
        default => "many",
    };

    try {
        $first = $names[0] ?? null;
        return $first ?? $kind;
    } catch (\Throwable $error) {
        return $error->getMessage();
    }
}
