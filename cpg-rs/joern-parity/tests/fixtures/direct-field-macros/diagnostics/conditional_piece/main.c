struct Item { int len; int width; int values[4]; };
#define Width width
#define Len len ? p->Width : index
int f(struct Item *p, int index) { return p->Len; }
