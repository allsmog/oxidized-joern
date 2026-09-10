struct Item { int Len; };
#define Len Len
#define READ(p) ((p)->Len)
int read(struct Item *p) { return READ(p); }
