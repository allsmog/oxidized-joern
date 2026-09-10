struct Item { int len; int width; int values[4]; };
#define Len values[1]
#define Width values[2]
int f(struct Item *p, struct Item *q) { return p->Len + q->Width; }
