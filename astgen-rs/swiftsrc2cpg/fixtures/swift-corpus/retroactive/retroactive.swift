protocol Drawable {}

extension Int: @retroactive Drawable {}

extension String: @retroactive Equatable, @retroactive Drawable {}
