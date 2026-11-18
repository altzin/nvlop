# nvlop
Qwen2.5 coder project to generate tests for pushed code local model, local infra.

I write code
I push code
worker stores diffs in redis
worker responds
Push returns succesful

async create PR to append tests at some later point.

also rolling code state tree sitter
