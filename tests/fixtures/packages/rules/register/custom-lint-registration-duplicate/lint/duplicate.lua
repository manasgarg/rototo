function register(lint)
  lint:rule({
    id = "payments/noop",
    title = "No-op rule",
    help = "No-op rule used by tests.",
    handler = "check_package",
  })
  lint:rule({
    id = "payments/noop",
    title = "No-op rule",
    help = "No-op rule used by tests.",
    handler = "check_package",
  })
end

function check_package(package, target)
  return {}
end
