double identity(double value) { double local = value; return local; }
double declared(double value);
double call_definition(double value) { return identity(value); }
double call_prototype(double value) { return declared(value); }
