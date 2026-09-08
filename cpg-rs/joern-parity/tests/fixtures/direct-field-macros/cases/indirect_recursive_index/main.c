struct Item { int len; int width; int values[4]; };
#define Len values[Other]
#define Other Len
int f(struct Item *p) { return p->Len; }
