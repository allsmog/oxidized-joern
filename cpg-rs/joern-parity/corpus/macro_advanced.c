#define INNER(x) ((x) + 1)
#define OUTER(x) INNER(x)
#define STRINGIZE(x) #x
#define CONCAT(left, right) left ## right
#define FIRST(first, ...) first

int macro_advanced(int input) {
    int token_value = 3;
    const char *text = STRINGIZE(input);
    return OUTER(input) + CONCAT(token_, value) + FIRST(input, 11, 12);
}
