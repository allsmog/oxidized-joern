struct Item { int Len; };
#define Len(x) (x)
#define READ(p) ((p)->Len)
int read(struct Item *p) { return READ(p); }
