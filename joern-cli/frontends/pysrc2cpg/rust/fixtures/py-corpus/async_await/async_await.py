from typing import Iterable, Optional


async def fetch(source: Iterable[int]) -> Optional[int]:
    total = 0
    async for item in source:
        total += await coro(item)
    async with opener() as handle:
        return handle.read()


async def coro(x):
    return x


async def opener():
    return open("x")
