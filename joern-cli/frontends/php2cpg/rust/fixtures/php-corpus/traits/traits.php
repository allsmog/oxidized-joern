<?php

trait Loud
{
    public function shout(): string
    {
        return "LOUD";
    }
}

trait Quiet
{
    public function shout(): string
    {
        return "quiet";
    }

    public function whisper(): string
    {
        return "...";
    }
}

class Speaker
{
    use Loud, Quiet {
        Loud::shout insteadof Quiet;
        Quiet::shout as protected murmur;
    }
}
