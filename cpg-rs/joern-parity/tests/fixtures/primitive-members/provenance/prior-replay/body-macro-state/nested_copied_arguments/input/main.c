#define VALUE 2
#define TWICE(x) ((x) + (x))
int choose(int value) {
 int first = TWICE(VALUE + value);
#undef VALUE
#define VALUE 7
 return first + TWICE(VALUE + value);
}
