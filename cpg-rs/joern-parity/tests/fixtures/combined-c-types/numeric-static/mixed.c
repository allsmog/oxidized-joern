static unsigned long converted(unsigned long value) { return value; }
static unsigned long declared(unsigned long value);
unsigned long calls(unsigned long value) { return converted(value) + declared(value); }
static unsigned long literal(void) { return 1ULL; }
static long double floating(void) { return 0x1p2L; }
static _Bool boolean(_Bool value) { return value; }
static volatile unsigned long qualified(volatile unsigned long value) { return value; }
static int (parenthesized)(int value) { return value; }
