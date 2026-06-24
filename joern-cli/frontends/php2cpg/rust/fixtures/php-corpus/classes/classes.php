<?php

interface Greeter
{
    public function greet(string $name): string;
}

class Service implements Greeter
{
    const VERSION = "1.0";

    private ?string $last = null;

    public function greet(string $name): string
    {
        $this->last = $name;
        return "hello $name";
    }
}

function run(?Service $service): string
{
    if ($service === null) {
        return "none";
    }
    $callback = function (string $value): string {
        return strtoupper($value);
    };
    return $callback($service->greet("x"));
}
