function register(lint)
  lint:rule({
    id = "invalid",
    title = "Invalid custom rule",
    help = "Invalid custom rule.",
    handler = "check",
  })
end

function check(package, target)
  return {}
end
