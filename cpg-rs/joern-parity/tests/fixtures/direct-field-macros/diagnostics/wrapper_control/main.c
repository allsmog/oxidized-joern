struct Item { int width; };
#define Sel p->width
int f(struct Item *p) {return Sel;}
