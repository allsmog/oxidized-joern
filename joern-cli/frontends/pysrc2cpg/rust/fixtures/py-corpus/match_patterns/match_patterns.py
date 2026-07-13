class Point:
    x: int = 0
    y: int = 0


def patterns(command):
    match command.split():
        case ["go", direction]:
            return direction
        case ["drop", *objects]:
            return objects
        case {"action": action, **rest}:
            return action, rest
        case Point(x=0, y=0):
            return "origin"
        case 1 | 2 | 3:
            return "small"
        case _:
            return None
