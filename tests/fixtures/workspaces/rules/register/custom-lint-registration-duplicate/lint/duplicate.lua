function register(lint)
  lint:rule({
    id = "payments/noop",
    title = "No-op rule",
    help = "No-op rule used by tests.",
    handler = "check_workspace",
  })
  lint:rule({
    id = "payments/noop",
    title = "No-op rule",
    help = "No-op rule used by tests.",
    handler = "check_workspace",
  })
end

function check_workspace(workspace, target)
  return {}
end
