package main

func source(value string) string {
	return value
}

func transform(value string) string {
	return value
}

func sink(value string) {
	println(value)
}

func main(user string) {
	raw := source(user)
	clean := transform(raw)
	sink(clean)
}
