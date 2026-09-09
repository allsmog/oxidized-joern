float identity(float value) { float local = value; return local; }
float declared(float value);
float call_definition(float value) { return identity(value); }
float call_prototype(float value) { return declared(value); }
