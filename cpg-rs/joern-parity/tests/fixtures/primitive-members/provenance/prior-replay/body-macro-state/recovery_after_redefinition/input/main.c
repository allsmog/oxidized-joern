#define BAD "ok"
#define VALUE 2
int consume(const char *, int);
int choose(void) {
#undef BAD
#define BAD "a" unresolved "b"
#undef VALUE
#define VALUE 4
 int first = consume(BAD, VALUE);
#undef BAD
#define BAD "fine"
 int second = consume(BAD, VALUE);
 return first + second + VALUE;
}
