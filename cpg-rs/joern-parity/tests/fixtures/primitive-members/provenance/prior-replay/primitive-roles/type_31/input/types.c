long double identity(long double value) { long double local = value; return local; }
long double declared(long double value);
long double call_definition(long double value) { return identity(value); }
long double call_prototype(long double value) { return declared(value); }
