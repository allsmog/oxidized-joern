int shared(int named);
int only_header(double);
int use_a(int x) { return shared(x); }
