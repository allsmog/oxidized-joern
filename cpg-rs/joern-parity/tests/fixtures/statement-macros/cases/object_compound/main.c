#define ACTION { use(value); }
void use(int v);
void f(int value) { ACTION; }
