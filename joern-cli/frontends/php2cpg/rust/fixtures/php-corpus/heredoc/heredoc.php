<?php

function banner(string $name): string
{
    $where = __CLASS__;
    $line = __LINE__;
    $message = <<<TEXT
Hello $name from {$where} at line $line
TEXT;
    $plain = <<<'RAW'
literal $name stays literal
RAW;
    return $message . $plain;
}
