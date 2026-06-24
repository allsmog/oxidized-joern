<?php

namespace App\Demo;

use App\Other\Helper;

const PREFIX = "app";

function build(Helper $helper): string
{
    return PREFIX . $helper->name();
}

class Widget
{
    public function render(): string
    {
        return \strtoupper(PREFIX);
    }
}
