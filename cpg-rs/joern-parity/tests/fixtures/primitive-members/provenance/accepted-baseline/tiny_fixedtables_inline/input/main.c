typedef struct Entry {
 unsigned char op;
 unsigned char bits;
 unsigned short value;
} Code;
struct State { const Code *len; const Code *dist; };
void fixedtables(struct State *state) {
static const Code lenfix[2] = {{1, 2, 3}, {4, 5, 6}};
static const Code distfix[1] = {{7, 8, 9}};
 state->len = lenfix;
 state->dist = distfix;
}
