function register(lint)
  lint:rule({
    id = "policy/conflict",
    title = "Policy conflict A",
    help = "Policy conflict A.",
    handler = "check",
  })

  lint:rule({
    id = "policy/conflict",
    title = "Policy conflict B",
    help = "Policy conflict B.",
    handler = "check",
  })
end

function check(package, target)
  return {}
end
