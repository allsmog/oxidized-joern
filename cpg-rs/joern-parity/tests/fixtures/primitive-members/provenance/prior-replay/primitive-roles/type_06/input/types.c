signed short int identity(signed short int value) { signed short int local = value; return local; }
signed short int declared(signed short int value);
signed short int call_definition(signed short int value) { return identity(value); }
signed short int call_prototype(signed short int value) { return declared(value); }
