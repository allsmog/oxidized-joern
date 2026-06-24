GREETING: str = "hi"


class Point:
    x: int = 0
    y: int = 0

    def move(self, dx: int, dy: int) -> "Point":
        self.x += dx
        self.y += dy
        return self


class Container[T]:
    def __init__(self, item: T) -> None:
        self.item = item

    def get(self) -> T:
        return self.item
