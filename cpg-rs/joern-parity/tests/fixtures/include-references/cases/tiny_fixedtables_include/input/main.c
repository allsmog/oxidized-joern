typedef struct Entry {
 unsigned char op;
 unsigned char bits;
 unsigned short value;
} Code;
struct State { const Code *len; const Code *dist; };
void fixedtables(struct State *state) {
#include "fixed.h"
 state->len = lenfix;
 state->dist = distfix;
}
