function register(lint)
  lint:rule({
    id = "payments/missing-rule",
    title = "payments/missing-rule",
    help = "payments/missing-rule",
    handler = "check",
  })
end

function check(package, target)
  return {}
end
