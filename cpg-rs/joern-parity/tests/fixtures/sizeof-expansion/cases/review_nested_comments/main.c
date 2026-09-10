#define S(x) sizeof ( /* outer */ (( /* inner */ x /* end */ )) /* tail */ )
int read(int value) { return S(value); }
