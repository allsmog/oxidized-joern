typedef struct Value { int value; int tag; } Value;
#define SET(L,p,v) { Value *slot=(p); slot->value=(v); slot->tag=1; use(L); }
void use(int v);
void f(int L, Value *p, int v) { SET(L,p,v); }
