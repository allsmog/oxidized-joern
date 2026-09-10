#define BAD "a" unresolved "b"
#define VALUE 3
int consume(const char *, int);
int choose(void) {
 int result = consume(BAD, VALUE);
 return result + VALUE;
}
